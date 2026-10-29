# ADR 27: Streaming Input - Reading a Grammar's Input from a `Read`

**Status:** Accepted. §1 (stage 0) is implemented; §2 and §3 are planned, in
that order; §4 records what is deferred or rejected. **Date:** 2026-10-29.
**Tests:** `tests/reader_test.rs` (stage 0).
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
fn parse_<rule>_reader<S>(
    reader: impl std::io::Read,
    context: &ParseContext<S>,
    how: rt::Parallelism,
    reading: rt::Reading,
) -> Result<T, ParseError>
```

and `parse_<rule>_reader_with(reader, new_context, how, reading)`, as for
`_pieces` (ADR 19). `rt::Reading { block, max_frame }` sizes the read and
bounds the memory: the buffer holds at most `block + max_frame` bytes, and an
input in which more than `max_frame` bytes follow the last boundary is an
error at that offset, not an allocation that grows with the input.

* **The answer is the sequential parse's.** A block is a piece in the sense of
  ADR 24 §1: it starts just past a boundary and ends just past one, so every
  frame lies in exactly one block. The results of the blocks are merged in
  order with the rule's merge, as `fold_pieces` merges pieces. A remainder
  left empty at the end is not parsed, as an empty piece is not.
* **D is part of it.** Each block is parsed with `fold_pieces` and the
  caller's `Parallelism`, so a block uses the cores as an input in memory
  does. Reading the next block while the current one is parsed is not done
  yet; it is a change inside `rt::fold_reader` and needs no new surface.
* **Text is checked per block.** A text grammar's block is checked as UTF-8
  before it is parsed - the check `&str` input implies, made where the bytes
  arrive. A block ends just past a boundary, which is whole characters, so the
  check never splits a character. A bad byte is the ADR 25 D5 error at its
  absolute offset. A byte grammar's block is not checked.
* **Errors are at their absolute offset.** `render` needs the source, which a
  stream no longer has; `offset` and the message are what the caller gets.
* **What the fold returns must not borrow the input.** A block lives for one
  step. `_reader` is generated only for a rule whose return type does not name
  `'a`; a rule that returns views keeps `_pieces` and has no `_reader`. Names
  outlive the block as symbols (`intern(p)`), anything else as a copy.
* **I/O errors** become a `ParseError` whose message names the I/O error, at
  the offset reached.

What stage 0 is not: it does not cover a format whose cut points are not a
byte string - YAML's documents are such a format (below) - nor a single large
element. Those are §3 and §4.

## 2. Measure `Partial` on the complete path - planned

Before anything takes `Partial`, measure what `Partial<&str>` with
`partial = false` costs a parse of complete input: every edge check becomes a
predictable branch on a field. If it costs nothing measurable, there is one
code path, every grammar can stream, and there is **no declaration** - whether
to stream is the caller's choice, as parallelism is (ADR 24 §3). If it costs,
the declaration is orthogonal to `input bytes;` (`stream;` beside it) rather
than a list in one declaration. A switch per entry rule is not needed either
way: entry points are per rule already.

## 3. A1 with the path designed in - planned

A1 restarts between the elements of a repetition. It is designed with a
**path**: the repetition may sit at the top of the entry rule
(`prefix ELEM* suffix`, a path of length 0) or be one marked repetition further
down, reached through a static chain of rules. The second is the typical large
document - one JSON array of records, one XML root with millions of children,
one YAML document whose top is a sequence - and is built in the same stage,
after the path of length 0.

What makes it sound, every point checked at compile time:

* **every scanner of the runtime and every inline fast path answers
  `Incomplete` at the edge of a buffer** - and a test feeds every input of the
  suite split at every byte offset, and in one-byte chunks, against the parse
  of the whole input;
* the values that reach the marked repetition come from parameters or from the
  prefix; a value one iteration hands to the next is a `fold`;
* the repetition's value is unbound, or it is a `fold`: its elements go to a
  callback (`for<'b> FnMut(Item<'b>)`), so the actions on the path never see a
  `Vec` of them;
* on the path, a cut precedes the repetition or no alternative is open:
  elements handed out cannot be taken back;
* an error in the suffix comes after elements already handed out; that is
  documented, not hidden;
* the diagnosing replay runs per element, from the element's checkpoint, while
  the element is in the buffer; an error carries line, column and an excerpt
  taken then; the default diagnose mode of a stream is `ReplayInPlace`, since
  `Replay` would clone `user_state` per element;
* an element longer than the limit is an error, not an allocation; the end of
  the stream is announced by a last call that marks the input complete.

**YAML is the test case for this stage**, not for stage 0: `STREAM = DOC*`
restarts between documents, and the grammar itself decides where one ends -
directives, `...` and the forbidden `---` at the start of a line included.

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
* **A declaration `input stream;` / `input complete, stream;`** - not now; §2
  decides whether one is needed at all.
* **The comparison in the request**, corrected: libyaml refills synchronously
  inside its scanner (`CACHE`, `yaml_parser_update_buffer`) and repeats
  nothing; Jackson's blocking parser pulls synchronously too, and only its
  non-blocking parser returns `NOT_AVAILABLE` from a hand-written state
  machine; with nom it is the caller that restarts.

## Consequences

* Stage 0 adds two functions to a `par_fold` rule whose type does not borrow
  the input, and `rt::Reading` and `rt::fold_reader`; nothing that exists
  changes.
* §3 touches every scanner of the runtime. Its differential test is a
  condition of the stage, not a follow-up.
