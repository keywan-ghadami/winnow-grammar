# ADR 27: Streaming Input - Reading a Grammar's Input from a `Read`

**Status:** Accepted, implemented: §1 (stage 0), §2 (measured), §3 (built,
without `Partial` - see there). §4 records what is deferred or rejected, §5
the numbers. **Date:** 2026-10-29.
**Tests:** `tests/reader_test.rs` (stage 0), `tests/stream_test.rs`,
`tests/stream_yaml_test.rs`, `tests/stream_extern_test.rs`,
`tests/stream_diagnose_test.rs`, `tests/stream_each_test.rs`,
`tests/stream_memory_test.rs`,
`tests/ui/reader.rs`, `tests/ui/stream.rs`; the whole suite once more with
`WINNOW_GRAMMAR_EDGES=1` (`scripts/test-edges.sh`). **Measured:**
`benches/stream.rs`, and callgrind (`scripts/profile-stream.sh`, §5).
§6 is for whoever changes this: where it lives, what a change must keep,
how to check and measure it.
**Raised by:** a feature request for input that arrives piece by piece - from a
pipe, a socket, or a file larger than memory - parsed in memory that stays
bounded however long the input is. Its first case is a YAML stream of many
documents; its second, the usual large YAML file, is one document with a
sequence of millions of entries at the top.
**Related:** ADR 17 (fast pass and replay), ADR 20 (the state is not rolled
back), ADR 22 (the interner), ADR 24 (frames, `par_fold`, the frame check and
its open flank §5), ADR 25 (byte input), ADR 26 (values in the grammar).

## Context

Every entry point takes the whole input as one slice, and `rt::finish` turns
winnow's `Incomplete` into an error. The request proposed:

* **A1** - admit winnow's `Partial`, let `Incomplete` reach the entry point, and
  add an entry point over `Read` that refills and resumes between the elements
  of the entry rule, with absolute positions and a length limit;
* **A2** - resumption points at every level, derived by the generator from the
  grammar: the stack of rules with their values (ADR 26), only at safe points
  (behind a cut, or with no alternative open), with `state`, the interner and
  `fold` reset on a retry;
* **B** - a buffer of blocks in which a view keeps its block alive, so views
  outlive the next step without a copy;
* **C** - a recognise-only version of every rule, to skip what is not used;
* **D** - parsing frames from a reader in parallel;
* the choice by a declaration, like `input bytes;`: `input stream;`,
  `input complete, stream;`.

Checked against the tree, three things weigh more than the request assumed.

**winnow's mechanism is only half of it.** The hot paths of this crate do not
run through winnow's primitives but through its own scanners, and those do not
know `Partial`. `rt::scan_to_literal` consumes to the end of the input when it
finds no terminator: at the end of a buffer `until(";")` would yield half a
name and no error. `first_byte`, `lit_byte`, the indexed runs (ADR 24 §8), the
word-at-a-time class scan, `dec` and the trivia memo of `skip_trivia` have the
same shape, as does the inline code from `codegen/fixed.rs` and
`codegen/expr.rs`. Every one of them must answer `Incomplete` at the edge of a
buffer, and one that is missed gives a *wrong result*, not an error.

**Diagnostics assume the whole input.** The replay of ADR 17 parses again from
the start of the entry point, `Diagnose::Replay` clones `user_state` before
every fast pass, and `ParseError::render` takes the source.

**Views borrow the input.** `&'a str` is tied to a buffer that moves when it is
refilled.

## 1. Stage 0: a reader over frames - built

A `par_fold` rule (ADR 24 §2) already says where its input may be cut, and the
frame check has proved it at compile time. Read a block, cut at the last
boundary in it, parse what lies before the cut with the parser the rule already
has, keep the rest for the next block: that is a stream, and it needs no
`Partial`, no change to any generated parser and no declaration.

Next to `parse_<rule>_pieces`, a `par_fold` rule gets

```rust,ignore
fn parse_<rule>_reader<__WgS>(
    reader: impl std::io::Read,
    context: &ParseContext<__WgS>,
    how: rt::Parallelism,
    reading: rt::Reading,
) -> Result<T, ParseError>
```

and `parse_<rule>_reader_with(reader, new_context, how, reading)`, as for
`_pieces` (ADR 19). `rt::Reading { block, max_frame }` sizes the read and
bounds the memory: the buffer holds at most two `max_frame` and a block - the
frame being waited for, the start of the line an error would be on, and the
read - and an input in which more than `max_frame` bytes follow the last
boundary is an error at that offset, not an allocation that grows with the
input. `tests/stream_memory_test.rs` counts the heap: 16 MiB of rows peak at
the same 192 KiB as 4 MiB.

* **The answer is the sequential parse's.** A block is a piece in the sense of
  ADR 24 §1: it starts just past a boundary and ends just past one, so every
  frame lies in exactly one block. The results of the blocks are merged in
  order with the rule's merge, as `fold_pieces` merges pieces. A remainder
  left empty at the end is not parsed, as an empty piece is not.
* **D is part of it.** Each block is parsed with `fold_pieces` and the
  caller's `Parallelism`, so a block uses the cores as an input in memory
  does. Reading the next block while the current one is parsed was measured
  and is not done (§4).
* **Text is checked per block.** A text grammar's block is checked as UTF-8
  before it is parsed - the check `&str` input implies, made where the bytes
  arrive. A block ends just past a boundary, which is whole characters, so the
  check never splits a character. A bad byte is the ADR 25 D5 error at its
  absolute offset. A byte grammar's block is not checked.
* **Errors are where they are in the stream.** The offset is the stream's,
  items are numbered across blocks (and, since this stage, across the pieces
  of `_pieces`, which numbered each from 1: a piece hands out how many items
  its fold accepted, which `ParseContext::fold.seen` held anyway, and a later
  piece's error is renumbered by the count before it). `render` needs the
  source, which a stream no longer has, so an error carries an `Excerpt` -
  line, column and the text of the line, taken while it was in the buffer -
  and `render_excerpt()` renders it exactly as `render(source)` renders the
  same error in memory. Lines are counted as the buffer drops them; the count
  is a vector loop (§5).
* **What the fold returns must not borrow the input.** A block lives for one
  step. `_reader` is generated only for a rule whose return type does not name
  `'a`; a rule that returns views keeps `_pieces` and has no `_reader`. Names
  outlive the block as symbols (`intern(p)`), anything else as a copy.
* **I/O errors** become a `ParseError` whose message names the I/O error, at
  the offset reached.

What stage 0 is not: it does not cover a format whose cut points are not a
byte string - YAML's documents are such a format (below) - nor a single large
element. Those are §3 and §4.

## 2. `Partial` on the complete path - measured

`Partial<&str>` with `partial = false` costs a parser of winnow's own nothing
measurable on complete input: 366 MiB/s against 359 for `&str`
(`benches/stream.rs`, `partial`). The question was asked because `Partial`
would have been the mechanism of §3. It is not - §3 found one that needs no
new input type - so the answer decides no code. What it would have decided is
settled the other way round: there is no `input stream;`. A rule is marked
`#[stream]`, the caller decides whether to call its reader, and only a grammar
with such a rule pays for what streaming needs (§3, §5).

## 3. A fold reached through a path - built, without `Partial`

### The mechanism: a parse that notes when it touched the end

`Partial` asks every parser to say "I need more" at the edge of a buffer. That
is the audit the request underestimated: the runtime's own scanners, the
indexed runs, the inline fast paths - and a parser that is missed gives a
wrong result. What streaming needs is less: to know whether a parse **could**
have gone differently with more input. That is a property of where its
parsers stopped, and it can be noted from outside them.

In a grammar with a `#[stream]` rule every parser the generator writes - every
node, not only the leaves - is wrapped in `rt::edge(k, p)`, which sets
`ParseContext::touched` when `p` succeeded within `k` bytes of the end of its
input, or failed where it began within `k` bytes of it. `k` is the furthest
any parser of the grammar looks past where it stops, or past where it began
when it fails: the longest literal, and per built-in what it reads - four
bytes for a UTF-8 character, six for `char`, eight for a float's `infinity`,
five for `false`, two for a signed integer or a line ending. The argument:
every position a parse reaches it reaches through a parser that stopped
there, which checked `k` bytes; whatever looks at most `k` bytes on was checked
already. Only `string`'s search for its closing quote fails further away, and
it is wrapped in `rt::edge_quoted`, which checks for an unescaped quote. The
indexed runs of ADR 24 §8 read past every wrapper and are not generated in
such a grammar. A hand-written parser that looks further calls `rt::touch`.

A result whose parse did not touch the end is the result the whole input
gives. One that did is thrown away and parsed again over more input - or,
when the window already holds `max_frame + k` bytes, reported as looking
further ahead than the bound on memory allows. Nothing about winnow's input
type, the generated parsers' signatures, or a grammar without `#[stream]`
changes; with `WINNOW_GRAMMAR_EDGES=1` the whole suite runs once more with
every grammar wrapped, and every result stays what it was.

### The path

`#[stream]` marks a rule that reaches one `fold(ITEM, init, step)`: the rule
holds it, or holds one call of a rule that does, and so on - a **path**,
found by the model (`winnow-grammar-model/src/stream.rs`) and checked:

* every rule on it has one alternative, no generics, no span or label over
  its body; nothing is open around the fold, so an item handed out is never
  taken back;
* exactly one element of each rule continues the path;
* the item is a call of a rule of the grammar, whose declared type is what
  the step receives - and may borrow the input: the step runs while the item's
  input is there;
* the fold's value is used only by the action;
* what crosses a stop - a value bound before the element the path continues
  in and used after it, or bound after and used by the action - has a type
  that is known (a rule's declared type, a built-in of fixed type) and does
  not borrow the input; so do the rules' parameters and return types.

`tests/ui/stream.rs` pins each refusal. The generator cuts the path into
**steps**, each a function generic over the input's lifetime and the error
type like any rule's: a prefix per rule (its elements before the path's,
returning what crosses and the next rule's arguments), the item, and a suffix
per rule. `parse_<rule>_reader(reader, &mut context, reading, params…)` runs
them through `rt::stream::Stream`, with the fold's loop, its step and every
action of the path in between, where no view into the input is alive - so
the input before the step is dropped and the next block read.

A fold below the top is the same stage: `SEQDOC -> TOPSEQ -> SEQ_COUNT` in
`tests/stream_yaml_test.rs` passes the indentation it detected down as an
argument (ADR 26), and `{"title": …, "items": [ … ], "tail": 7}` in
`tests/stream_test.rs` keeps the title across the items.

### Steps, windows, state

* **One window serves every item it decides.** `Stream::items` parses items
  until one touches the end of the window, then reads and goes on from that
  item. One step per item cost 78 ns of bookkeeping on a 14-byte row; one
  window for all of them made the reader 2.5 times faster (§5).
* **Parsed again only where nothing is lost.** With a user state of size 0 -
  `()`, the common case - an item that touched the end is simply parsed
  again over more input: nothing it did is left over. With another state the
  window is filled to `max_frame + k` first, so every item gets all of it and
  is never parsed twice; `Diagnose::Replay`, which keeps a copy anyway, is
  parsed lazily like a state of size 0. Reading asks for one read at a time,
  so an item that has arrived is parsed before the next read blocks.
* **Positions are the stream's.** Errors, what the diagnosing pass recorded
  (merged outside the window, so that an attempt that began before it keeps
  its place in the ranking of ADR 15), recovered errors, spans bound with `@`
  (`ParseContext::origin`, added only in a grammar with a `#[stream]` rule).
* **The text is checked where it arrives**, as UTF-8 up to the last whole
  character; a window never ends inside a character, and a bad byte is the
  error of ADR 25 D5 at its stream offset.

### Errors, as the parse in memory reports them

The parse in memory replays a failed fast pass from the start. A stream
cannot, and does not need to: the failure that wins on progress is recorded
at or near where the parse stopped. When the fold ends or an item fails, the
item before it - kept in the buffer for this - is parsed again by the
diagnosing pass for what it records, without its step and with the state put
back, then the failing item; the suffixes and the end check merge with what
that recorded. The comparison with the parse in memory - value, or error
offset, message and `render_excerpt()` against `render(source)` - is the test
of every stream test, read a byte at a time, in odd sizes and whole, under
every `Diagnose` mode.

**Items further back.** An item can record a failure past the items after it:
`ITEM = "a" "a" "a" "!" | "a"` tries four bytes at every `a` and keeps one, so
the attempt at the first `a` of `aaab` fails at the `b` three items on - and
in memory, begun earliest, it is the message (`expected `!``). Such a failure
is recorded only where a repetition, an option, a `fold`, a `recover` or a
rule's alternatives swallow it, and it lies past the item's end only when the
attempt given up could read more than one token first. The generator decides
that per grammar (`codegen::edge::reaches`): in what the stream rules reach,
outside the stream's own fold, is there a swallowed attempt that is not a
literal or a built-in token, or a rule alternative that can fail after its
first element? Rows, `n:i64 ","?`, the `Records` grammar: no - they are
generated as before. YAML, `Far`: yes - their parsers are wrapped in
`rt::edge_reach` instead, which also notes how far a failure got
(`ParseContext::reach`), and the driver keeps every item whose failures got
within `k` of the next step, in the buffer and in a list, and replays them in
order before the item before the failure. An item drops out of the list once
the parse has moved past what it reached, so the buffer stays bounded.

Found on the way, and fixed: under `Diagnose::Eager`, which records in the
first pass, an item that touched the end of its window kept what it had
recorded over the partial input when it was parsed again over more; its
recordings are now put back with the rest of its state.

`tests/stream_diagnose_test.rs` has both: the cases above, an option and a
repetition whose attempt reads two tokens, a `not(…)` that looks across four
items, built-in tokens under `?` (`u8` that overflows, `f64`, `raw_ident`) in a
grammar the analysis leaves untracked, every input up to six or seven bytes
over each grammar's alphabet, and 200 000 reaching items through a 4 KiB
window. Turning the analysis off, emptying the list, or dropping the eager
restore each fails a test.

**Cost.** Tracking in every wrapper, measured first, cost 9-12 instructions a
row on every parse of such a grammar, in memory too; with the analysis a
grammar that does not need it pays nothing for the wrappers. The driver's
loop takes the decision as a constant (`Stream<…, TRACK>`). Rows read by
`#[stream]`: 265 instructions a row before, 268-272 after, the spread being
inlining that moves from build to build; in wall time no difference beyond
the noise of the machine on rows, and the list ran faster.

### The caller sees the items: `#[stream(each)]`

The fold's step is grammar text and sees only `(acc, item)`: it cannot reach
the caller, a channel, or the context. So control over the items belongs to
the reader, not to the fold: `#[stream(each)]` adds
`parse_<rule>_reader_each(…, each: impl FnMut(&Item) -> ControlFlow<__WgB>)`
returning `Result<ControlFlow<__WgB, T>, ParseError>`.

* **The tap observes, it does not take.** `each` sees the item by reference
  inside the item step, where `commit` holds and before the fold's step; the
  fold runs as without it. Taking the item instead would leave the
  accumulator at `init()`, and the rule's value would then depend on how it
  was called.
* **Once each.** `commit` is false in the diagnosing replays (`Pass::replay`)
  and for an item that touched the end of its window, so neither a retry nor
  a diagnosis shows an item twice.
* **`Break` is `Fail::Stop`**: the item counts as taken, the driver stops -
  never diagnosed, retried or replayed - and the generated code returns
  before the suffixes and the end check, so what follows is neither read nor
  checked.
* **Only on request.** The function is generated for a rule marked `each`;
  a stream rule without it, and every other grammar, generate what they did.

Considered and not done: a `try_fold` whose step returns `ControlFlow` - in
memory the fast pass's `EmptyError` cannot carry the user's value, and a step
runs again when `Replay` re-parses after a cut, so a step with an effect
would act twice; and a real iterator, which needs the path as a generated
state machine - a second code generator, the objection of §4 to A2. A thread
and a bounded channel over `_each` give an iterator today (SYNTAX.md).

`recover(…)` inside an item is tested against the parse in memory too -
value, `recoveries`, and `recovered` under `Eager` - and a skip longer than
`max_frame` is the too-far error. A cut is not recovered from, there as
anywhere: an item grammar with `=>` stops the stream at the first record
broken after it. Whether a stream should diagnose a recovered item by itself
under `Replay` - cheap here, impossible in memory, and so a divergence from
the parse in memory that would have to be asked for - or whether a
combinator that recovers from a cut is wanted, is left to the first real item
grammar that needs one (TODO §8).

`tests/stream_each_test.rs`: every item once and in order under every
`Diagnose` mode, read a byte at a time and whole, with a user state of size 0
and not; `Break` reads no further than its window and checks nothing after
it; the channel iterator; `recover` in an item. Moving the tap before the
`commit` check fails four of its tests.

### How the edge tracking is tested

`tests/stream_test.rs` parses every kind of parser as the items of one fold
- literals that are prefixes of one another, keywords, floats, integers,
strings with escapes, characters, `bool`, `until`, `not`, `peek`, `count`,
`text`, spans, `ident` - over a corpus, 1 500 random inputs and every input up
to four characters over a nine-letter alphabet, and each built-in alone in a
grammar of its own, so that its own entry of `k` is the grammar's. Lowering
that entry, or dropping `edge_quoted`, makes the test fail: every entry of
the table has a test that needs it.

### YAML

**YAML is the test case of this stage**, not of stage 0: `STREAM` folds over
documents, and the grammar itself decides where one ends - directives, `...`
and c-forbidden, so that `---x` at the start of a line is content and `---`
is a marker. `tests/stream_yaml_test.rs` reads a stream of documents and one
document whose top is a sequence, on every prefix of each, against the parse
in memory.

### Why the frame check rejects a YAML document framed by `---`

Probed with the grammar of `tests/yaml_indent_test.rs`, extended to documents
under `#[frame(boundary = "---\n")]` and `par_fold`, with `not(FORBIDDEN)`
before every scalar: rejected at the literal `"-"` of `SEQ_ENTRY`, and without
it at every `"\n"`. With `unchecked` it compiles. The reasons, from the surface
down:

1. a boundary of more than one character is treated conservatively: a literal
   that shares any character with it is rejected (ADR 24 §4), so every `-`,
   every `"\n"` and `line_ending` go;
2. YAML protects the marker by a lookahead (c-forbidden: no `---` or `...`
   followed by white space at the start of a line), not by an alphabet. The
   check proves locally that a construct cannot consume the boundary; it does
   not prove that every `\n` consumed is followed by a `not(…)`. That would be
   an analysis of what may follow a character - TODO §1's position-sensitive
   reachability, larger;
3. the values of ADR 26 do not help: the check reads a guard as the empty
   pattern and does no arithmetic over values, and at the top `n = -1`, so
   content may sit in column 0 - which is why the specification needs
   c-forbidden as a rule of its own;
4. the frame model does not fit: a frame ends in its boundary, a YAML document
   begins with its marker; `---` is a marker only when white space, a line
   end or the end follows (`---x` in column 0 is a scalar); directives belong
   to the document after them. `unchecked` therefore compiles but does not
   make the cut right: a boundary that is a byte string cuts at `---x` too.

The check stays as it is. The keys ADR 24 §5 already reserves - `start`, and a
condition on what follows the boundary - are of general use (log records that
begin with a time stamp) and are added when parallel cutting (D) needs them, not
for YAML.

## 4. Deferred, rejected

* **Reading the next block while this one is parsed - measured, not done.**
  Reading from memory runs at 12 GiB/s against a parse of 0.5-0.8 GiB/s: at
  most a few per cent to win, for a thread and a `Send` bound on every reader.
  A source slower than the parse is the case where it would pay, and there
  the parse waits for the source either way.
* **A2 in full - deferred.** Resuming at every level turns the native call
  stack into an explicit state machine: a second code generator, at odds with
  the inlining and the fixed runs of ADR 23 and 24. Resetting `state` and the
  interner contradicts ADR 20 (a state write after a cut stays) and ADR 22 (a
  shared interner has handed its symbols out). "Where no point is safe, only
  the buffer grows" breaks the bound on memory silently; under ADR 24 §1 that
  has to be a checked claim. The marked path of §3 covers the case that was
  asked for.
* **B - rejected.** A `&'a str` cannot keep a block alive; B needs a view type
  of its own in every action, and works against a bounded memory. A copy or
  `intern(p)` serves the case.
* **C - deferred.** Skipping actions that write `state`, intern or fold changes
  what a parse means; `until` and frames already skip fast. Built only when a
  measurement shows that building values dominates - and a user's own code
  generator can emit a recognising grammar itself.
* **A declaration `input stream;` / `input complete, stream;`** - not needed
  (§2): `#[stream]` marks the rule, the caller chooses the reader.
* **`Partial`** - not needed (§3), though it would have cost nothing (§2).
* **The comparison in the request**, corrected: libyaml refills synchronously
  inside its scanner (`CACHE`, `yaml_parser_update_buffer`) and repeats
  nothing; Jackson's blocking parser pulls synchronously too, and only its
  non-blocking parser returns `NOT_AVAILABLE` from a hand-written state
  machine; with nom it is the caller that restarts.

## 5. Numbers

`benches/stream.rs`, one machine with four shared cores, so the table below
it is the reliable one; 400 000 rows of `station12;345\n` and a list of
200 000 integers:

| | in memory | read |
|---|---|---|
| rows, `#[stream]` | 709 MiB/s | 449 MiB/s |
| rows, `par_fold` | 788 MiB/s | 611 MiB/s |
| list `[1, 2, …]`, `#[stream]` | 152 MiB/s | 115 MiB/s |
| reading alone | | 12 GiB/s |

Callgrind, instructions per row, 200 000 rows (`scripts/profile-stream.sh`):

| | per row |
|---|---|
| the fold in memory, no edge tracking | 176 |
| the same with edge tracking | 200 (+14 %) |
| its `#[stream]` reader | 266 (+33 %) |
| `par_fold` in memory | 176 |
| its reader | 203 (+15 %) |

What went into the readers' share: counting lines for excerpts was 3.5
instructions a byte as `filter().count()`, 0.4 as a count in `u8` over runs of
255 bytes; one window per item was 78 ns per row of bookkeeping, one window
for as many items as it decides ~30 instructions. The edge tracking costs
14 % on rows and 20 % on the list in wall time (`edges`), and a grammar pays it
only when it has a `#[stream]` rule.

## 6. For whoever changes this

### Where it lives

| | |
|---|---|
| `winnow-grammar-model/src/stream.rs` | `#[stream]`: finds the path, checks it, says what crosses each stop (`Streams`, `StreamPath`, `Level`) |
| `winnow-grammar-macros/src/codegen/edge.rs` | `k` of a grammar (`lookahead`), whether it needs `edge_reach` (`reaches`), the wrapper, `origin()` for spans |
| `winnow-grammar-macros/src/codegen/stream.rs` | `parse_<rule>_reader`: the path cut into prefix, item and suffix steps |
| `winnow-grammar-macros/src/codegen/rule.rs` | stage 0's `parse_<RULE>_reader(_with)` (`generate_reader_fns`) |
| `src/rt/stream.rs` | `edge`, `edge_reach`, `edge_quoted`, `touch`, `span_at`; the driver `Stream` (windows, retries, positions, the diagnosing replay and the items it keeps for it) |
| `src/rt/reader.rs` | `Reading`, `Buf` (the read buffer, line counting, excerpts), stage 0's `fold_reader` |
| `src/error.rs` | `Excerpt`, `render_excerpt` |

`codegen::Codegen::streaming` is the one switch: true in a grammar with a
`#[stream]` rule (or under `WINNOW_GRAMMAR_EDGES`), and every difference in
generated code hangs off it - `edge()`, `origin()`, `span_moved()`, and the
fixed runs of ADR 24 §8 turned off.

### What a change must keep

- **Every parser the generator writes goes through `generate_parser_expr`**,
  which wraps it in `edge()`. A new construct that builds a parser elsewhere
  (as delimiters and `recover`'s skip do) wraps it itself.
- **A new construct that swallows a failure** - one that records it with
  `record`/`record_if_begun` and goes on - is a swallowing point for
  `edge::reaches`, and a new built-in that can fail after it began is not
  `atomic` there. Otherwise a grammar is generated without `edge_reach`, and
  an error further on misses what that construct recorded
  (`tests/stream_diagnose_test.rs`).
- **A new built-in gets its entry in `edge::lookahead`** - how far it looks
  past where it stops, or past where it began when it fails - and a
  `single_<name>` grammar in `tests/stream_test.rs`, so that its entry is the
  grammar's `k` and lowering it fails a test. A built-in whose scan has no
  bound (`string`) needs a check of its own, as `edge_quoted` is.
- **A parser that reads past the wrappers** - an indexed run, an inline fast
  path that looks ahead - is not generated when `streaming` is set, or notes
  the end itself (`rt::touch`).
- **A grammar without `#[stream]` generates what it did.** Nothing outside
  `if self.streaming` may change; the ordinary suite and the benches of
  ADR 24 hold that line.

### How to check a change

- `cargo test --workspace`, with and without `--features rayon`.
- `scripts/test-edges.sh`: the whole suite once more with every grammar
  wrapped. Every result must stay what it was; a test that changes says the
  wrapping changed a parse, not only noted one.
- `tests/stream_test.rs` compares every reader with the parse in memory,
  read a byte at a time - the read that cuts every item - in odd sizes and
  whole, under every `Diagnose` mode; a new case belongs there in the same
  form (`agree`). `tests/ui/stream.rs` holds every rejection of the path.
- `tests/stream_memory_test.rs` is the bound on memory; it uses a counting
  global allocator and must stay a test binary of its own.

### How to measure

- `cargo bench --bench stream` - wall time, the first table of §5. On a
  shared machine, read only differences larger than run-to-run noise.
- `scripts/profile-stream.sh` - instructions per row under callgrind, the
  second table, from `examples/stream_profile.rs`. This is the number to
  compare before and after a change of the driver or of the wrapping.

### Open

TODO §8: frame keys for parallel cutting. The per-item loop is measured
and left as it is (TODO §8).

## Consequences

* A `par_fold` rule whose type does not borrow the input has
  `parse_<rule>_reader`; a `#[stream]` rule has its own. `rt::Reading`,
  `rt::fold_reader`, `rt::fold_pieces_counted`, `rt::stream`, `rt::edge`,
  `rt::touch`, `ParseError::render_excerpt` and `Excerpt` are new;
  `ParseContext` gains two hidden fields and `ErrorCore` the excerpt.
* User documentation: `SYNTAX.md`, *Reading from a stream*, including what a
  hand-written parser owes a stream grammar; the README's short version.
* A grammar without `#[stream]` generates what it did.
* Items of `_pieces` are numbered across pieces.
