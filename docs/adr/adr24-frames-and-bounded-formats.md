# ADR 24: Frames — Parsing an Input in Pieces, and What a Bounded Format Buys the Parser

**Status:** Accepted. Part I is in force as implemented; Part II §8 is
implemented, §9 was measured and rejected. **Date:** 2026-10-04.
**Supersedes:** ADR 16, now withdrawn. Everything of it that still holds is
here, brought up to date (§2 and §3 had drifted from the code). Its sections 1
to 5 keep their numbers, so a reference to "ADR 16 §4" in the history reads as
§4 here. Its §5a is §6 and its §6 is §7.
**Tests:** `tests/frames_test.rs` (the split, split + parse + merge against the
sequential parse, the driver), `tests/ui/frames.rs` (what the check rejects),
`tests/fixed_run_test.rs` (§8: the indexed match against the element-by-element
parse on every string up to six characters). Where the tests and this ADR
disagree, this ADR wins.
**Measured on:** Nikaia's `examples/1brc.nika`, the workload that raised
Part II ([Nikaia#392](https://github.com/Nikaia-Language/Nikaia/issues/392));
§10 has the method.
**Related:** ADR 14 (the shared context), ADR 15 (a grammar states its
contract, the tooling checks it), ADR 17 (the fast pass and the diagnosing
pass), ADR 19 (`_pieces` and `_pieces_with`), ADR 21 (merging across pieces),
ADR 23 (deferred decoding).

# Part I — Frames

## Context

A parser reads its input front to back on one core. For a large, regular
input — a log, a measurement file, a data export — that is the whole cost, and
the machine has more cores than one. The obvious remedy is to cut the input
into pieces and parse the pieces in parallel. The obvious problem is that a
blind cut lands somewhere inside a record, and whether the two halves parse to
the same total as the whole depends on the data.

winnow-grammar's stance elsewhere (ADR 15) is that a grammar states its
contract and the tooling checks it. The same is wanted here: a grammar should
be able to *say* where it may be cut, and the claim should be *checked* at
compile time, so that a wrong total is a compile error and not a bug report
from a file that happened to have a space in the wrong place.

The first design did the checking, and additionally made the generated parser
conform: an `until(";")` reached from a `"\n"` frame was silently generated as
"until `;` or newline". The grammar did not say that. The same rule text then
parsed differently in two grammars, a rule shared between a frame and
something else changed behaviour for the something else, and the code
generator carried a per-rule mutable "current boundary" to do it. For a
feature whose whole point is that the grammar is believed only after it is
checked, an unwritten rewrite of the grammar is the wrong tool.

## 1. A frame is a claim, and the claim is checked — never repaired

A rule marked `#[frame]` claims that an occurrence of it can be found from any
byte offset by scanning forward to the next **boundary**: the literal the rule
ends in (inferred), or the one named in `#[frame(boundary = "\n")]`. Every rule
reachable from the frame is walked, and each construct that consumes input is
either **safe** or **rejected** with the rule and the pattern named:

| construct | verdict |
|---|---|
| a literal without the boundary in it; a built-in whose alphabet cannot include it (`digit1`, `ident`, …); lookahead | safe |
| `until(t)` whose terminator **covers** the boundary — one alternative is `frame_end`, the boundary literal or a prefix of it, or `line_ending` for a newline boundary | safe |
| a literal containing the boundary; a built-in that can consume it (`any`, `multispace0`); the implicit whitespace of a syntactic rule | rejected |
| `until(t)` that does not cover the boundary | rejected — the message says `until(… \| frame_end)` |
| `recover(…)` | rejected — the skip would have to stop at the boundary and the sync token would have to consume it; recover per frame with an alternative instead: `(item \| until(frame_end)) frame_end` |

A frame must end in its boundary, so that every frame ends exactly at one and
the piece that finds a frame's end is the one that owns it.

Nothing the check does changes what a pattern means. `until(";" | frame_end)`
says what the parser does, and it does that wherever the rule is used. This
costs the flagship grammar one alternative in one rule.

**`frame_end`** is the built-in that names the boundary of the enclosing
frame, so the boundary is written once — in the attribute — and referenced,
not copied. As a parser it matches the boundary; as an `until` alternative it
covers it by construction, so neither the check nor the generator has to
recognise that a literal happens to equal the boundary. It resolves
*statically*: the reachability walk assigns each rule the boundary of the
frame that reaches it, a rule that writes `frame_end` without being reached
from a frame is an error, and one reached from two frames with different
boundaries is an error — that rule is part of one frame's format. The literal
form (`until(";" | "\n")`) stays legal and means the same. The generator
keeps one piece of per-rule state for this, what `frame_end` stands for in the
rule being generated; it resolves a name the grammar wrote and never rewrites
a pattern the grammar did not.

`#[frame(…, unchecked)]` skips the walk. It is the `unsafe` of this feature:
the author asserts the invariant, and the word is greppable. It exists because
of §5.

## 2. `par_fold` is `fold` plus a merge, and it skips nothing

`par_fold(rule, init, step, merge)` folds over a frame rule and must be the
whole body of its rule. That rule skips no whitespace — not at its entry
point, not at its start, not between its items — whatever its name says: a
lowercase `par_fold` rule is lexical in all but name. Every other rule's entry
point skips whitespace around the input, and that was a wrong total in the
first design: the per-piece parser *is* the entry point, so the skip ran once
per piece instead of once per input, and a frame beginning with a space lost
the space in the piece that happened to start there. The same held one level
in, until `c61915e`, for the skip a syntactic rule makes at its start and
before every repeated item: at one piece a station ` b` lost its blank and a
blank line between rows passed, cut into pieces the blank stayed and the blank
line failed.

The property `par_fold` promises is that pieces and the sequential parse agree
on every input, including the ones both reject; a trailing blank line is
therefore an error in both rather than tolerated in one. Its items are
independent of one another and of any state, which is also what lets the
diagnosing pass replay only the item the fast pass stopped in (ADR 17,
`rt::entry_framed`).

## 3. Parallelism is the caller's to switch, size, or turn off

A `par_fold` rule generates, next to its parser:

- `frames_<rule>(input, n) -> Vec<Range<usize>>` — the cut (a `#[frame]` rule
  gets this one too);
- `merge_<rule>(a, b)` — the merge it was given;
- `parse_<rule>_pieces(input, &context, how)` — the driver: cut, parse every
  piece with a clone of `context`, merge in order, and report a failing
  piece's error at its offset in the whole input;
- `parse_<rule>_pieces_with(input, new_context, how)` — the same driver with a
  context *built* per piece, for a `user_state` that must start empty in every
  piece (ADR 19).

`how` is `rt::Parallelism`: `Off` (no cut, one parse — identical to
`parse_<rule>()`), `Pieces(n)`, or `Auto` (one per available core). Threads
come from the optional `rayon` cargo feature; without it the same driver runs
the pieces in sequence — the same cut and the same answer, which is what a
test uses to check the split without a thread pool. Thread count is rayon's
global pool's to configure; piece count is `Parallelism`'s. `frames_…` and
`merge_…` stay public so that a caller with its own executor needs nothing
from this crate but the cut and the merge.

Cloning one context into every piece shares its interner, because the
interner inside is an `Arc`: symbols from two pieces mean the same thing,
which is ADR 14's shape. Each piece parses through the rule's own entry, fast
pass first, diagnosing replay only for a piece that fails (ADR 17).

## 4. UTF-8 is a guarantee of the split, not a restriction on it

The split works on bytes: `rt::frames_bytes(&[u8], &[u8], n)`, with
`rt::frames(&str, &str, n)` as its `&str` view. A valid UTF-8 boundary can only
match at a character boundary, so every range is one; that is a consequence
of the input being `&str` and costs nothing. There is no knob for it because
there is nothing to trade: a `&str` cannot be sliced mid-character. Should a
byte-oriented input type arrive, the primitive it needs is already the one in
use. The one restriction that *is* a judgement call — the conservative overlap
rule that rejects any literal sharing a character with a multi-character
boundary — is overridden by `unchecked`, not by a second rule.

The same argument, one level down, is why a character class can be scanned as
bytes (ADR 23) and why §8 may cut a `&str` after an element it matched by
byte.

## 5. The open flank: what the frame definition cannot say yet

A boundary is a byte string. Formats exist whose cut points cannot be found by
looking for one:

- **quoted fields** — CSV, where a newline inside `"…"` is data. The scan
  would have to track quote parity (a prefix-xor over the piece, which SIMD
  does well, but it is a *stateful* pre-scan, not a search);
- **a start pattern** — records that end in a common byte but begin
  recognisably (`\n` followed by a digit, a timestamp, a sync word), where the
  right cut is "boundary followed by start" and the check would have to prove
  the start cannot occur mid-record;
- **escapes** — a boundary preceded by `\` that is not one;
- **a scanner of the author's own** — for anything else, a function that
  returns the cut points and is trusted the way `unchecked` is.

The attribute is a **keyed list** — `#[frame(boundary = "\n")]` — so that each
of these is another key (`quote = "\""`, `start = digit`, `escape = "\\"`,
`scan = my_fn`) and none of them changes what the existing keys mean. The
positional forms `#[frame = "\n"]` and `#[frame("\n")]` are rejected with a
pointer to the keyed one, so that no second positional meaning has to be
invented later. `rt::frames_bytes` is the primitive each of those scanners
would replace or wrap. Until one exists, a grammar for such a format uses
`unchecked` and says so in the grammar.

## 6. Where the check stops: opaque parsers are the author's responsibility

The walk reasons about what the *grammar* says. Three things it cannot see
into, and does not pretend to:

- a **hand-written parser reached by path** (`super::word`, `Alias::rule`) —
  the documented way to plug Rust into a grammar;
- an **`extern rule`** — a name and a return type, with the parser supplied
  elsewhere;
- **any bare name** in a grammar with a glob `use …::*`, because that already
  switches off the "undefined rule" check that would otherwise resolve it.

For these the check has nothing to walk: the parser is Rust, and whether it can
consume the boundary is a fact about code the grammar does not contain. So the
frame guarantee covers the grammar and stops at that edge — **whether such a
parser is safe to cut around is the developer's to know, not winnow-grammar's
to certify.** A `par_fold` over a frame that reaches one is only as sound as
that parser is.

There will be no key to declare it safe. A `consumes_no_newline` annotation
would read like a checked claim while being exactly the opposite — an
unverifiable promise, and a second one next to `unchecked`, which already says
"the author asserts this" for the whole frame and is greppable. One escape
hatch that is honest beats two that look like guarantees.

What the check *can* do at that edge it does: a rule the grammar defines is
walked, and a built-in it does not know is assumed to consume anything
(`builtin_may_consume`'s `_ => true`).

## 7. Where the check runs — once

`frame::check` runs in the model's validator, where its errors are reported.
The validator returns what it established (`validator::Validated`: the frames
and the grammar analysis), `parse_grammar` hands it on as `ParsedGrammar`,
and the code generator takes that whole. Nothing is analysed twice, and the
generator has no way to disagree with the validator about which rules are
frames. This is the shape to build from the start for any analysis that both
diagnostics and generation consume: one analysis, one result, passed on.

# Part II — What a bounded format buys the parser

## Context

The 1BRC grammar declares more than the generated parser used:

```text
rule NAME   -> &'a str = s:until(";" | frame_end) -> { s }
rule TENTHS -> i32     = neg:"-"? whole:digit{1,2} "." frac:digit -> { … }
#[frame(boundary = "\n")]
rule MEASUREMENT       = name:NAME ";" => temp:TENTHS frame_end -> { … }
```

Measured against a hand-tuned loop (Nikaia's `benches/brc`, `tuned`), the
difference was put down to two shapes
([Nikaia#392](https://github.com/Nikaia-Language/Nikaia/issues/392)):

- **Fixed width.** `"-"? digit{1,2} "." digit` is three to five bytes. The
  generated parser read it element by element — a repetition with a
  checkpoint per digit — where `tuned` indexes into the bytes.
- **One search per line.** `until(";" | frame_end)` is one `memchr2` per line,
  where `tuned` runs one `memchr` over the whole file and walks back from the
  end of each line to the `;`.

Each was built and measured on its own (§10). The first is kept; the second is
not, and why is the larger part of what this records.

## 8. A run of fixed-shape elements is matched by index

**Decision.** In a lexical sequence, wherever two or more of these stand next
to each other —

| element | its binding |
|---|---|
| a string or char literal (non-empty) | `&'a str` |
| `"…"?` | `Option<&'a str>` |
| the built-in `digit` | `char` |
| `digit?` | `Option<char>` |
| `digit{m,n}` with `n ≤ 16` | `&'a str` |

— the generator emits one indexed match over the unconsumed input's bytes
(`rt::rest`) and, if it matches, a single advance (`rt::advance`). The values
the bindings get are the ones the elements' own parsers would have produced,
cut from the same input. `codegen/fixed.rs`.

**Only the fast pass's accepting path is new.** Three things keep the
observable behaviour where it was:

1. *A failed indexed match changes nothing.* The input has not moved, and the
   elements run again exactly as they were generated before — so the failure,
   its position, and a `cut` after `=>` come from the code that produced them
   before.
2. *The diagnosing pass never takes the indexed path* (`E::RECORDING`). An
   optional element that did not match leaves "also possible here" behind in
   the context; the indexed match has no business knowing that, and the first
   version, which ran in both passes, lost exactly that note. The equivalence
   test found it on the empty input.
3. *Acceptance is the same.* Each element is greedy in both, and a sequence
   does not backtrack into an element that matched: `digit? digit{2}` on `12`
   fails both ways, because `digit?` keeps its digit.

`tests/fixed_run_test.rs` runs five rules twice — as written, and with `""`
between the elements, which matches everywhere and breaks every run — on every
string of up to six characters over `- 0 9 . x é ;` and requires the same
value, the same consumed length and the same rendered error.

**Cutting the `&str` is safe** for §4's reason: every element accepted ends on
a character boundary, an ASCII digit or a whole literal.

**Measured.** 676 → **608** instructions per row (−10 %), branch mispredictions
4.56 → 4.21 per row. On the clock it does not show: 0.426 s against 0.427 s,
pinned to one core, best of 25 over 8 million rows. §10 says why.

**Measured again downstream**, when Nikaia moved its pin to this commit
(Nikaia 0.0.415, both sides built by its own compiler, no `[patch]`): 587 →
**566** instructions per row (−3.6 %), 0.395 s → 0.390 s on one core, inside
the noise. The baseline is lower than above because Nikaia's own code had
moved on between the two measurements, and the share §8 removes is smaller
with it. Mispredictions went *up* there, 4.22 → 5.22 per row, and the one
that was added sits in Nikaia's action, `for d in whole.chars()`: whether a
temperature has one whole digit or two is a coin toss per row, and the branch
that pays for it moved from the repetition in the parser to the loop in the
action rather than going away. A value computed while matching - an
`int(digit{1,2})` capture - would be the way to remove it, and it is a
question for the language, not for this crate. Nikaia's compiler itself, on
its largest source file, is unchanged (382.54 M → 382.56 M instructions,
the same Rust out): its grammar has few runs of this shape.

It stays because it costs nothing at run time that it does not repay, removes
work rather than moving it, and the rule it applies is local — a run of
elements in one sequence, decided from those elements alone. The generated
parser of a rule still depends on nothing outside that rule.

## 9. One search per input instead of one per frame — measured, rejected

**What was built.** `rt::par_fold_framed`: over a frame with a one-byte
boundary, the fold ran one `memchr_iter` over its input, and before each item
wrote the end of that item's frame into the context. An
`until(";" | frame_end)` inside the item (`rt::scan_to_any_framed`) then
searched only for `;`, only up to that end. The answer is the same as before:
the end is the first boundary at or after the item's start, the scan starts
inside the item at or before it, so the first boundary from the scan is that
one.

**What it measured** (instructions per row, 1 million rows, with §8 in place):

| variant | per row | mispredicted branches per row | one core, 8 M rows |
|---|---:|---:|---:|
| §8 alone | 608 | 4.21 | 0.427 s |
| one `memchr_iter`, then `memchr` for `;` up to the end | 706 | 5.81 | **0.499 s** |
| the same, `;` found by a byte loop | 673 | — | — |
| one `memchr_iter`, `;` found *backwards* by `memrchr` | 684 | — | — |
| one `memchr_iter`, `;` found backwards by a byte loop — what `tuned` does | 649 | 5.48 | 0.419 s |

**Why it loses.** A line here is about fifteen bytes. A `memchr2` over fifteen
bytes finds both needles in the first vector it loads, so the per-line search
being replaced was already cheap — about the cost of one step of the iterator
that replaces it. Splitting one two-needle search into a one-needle iterator
plus a one-needle search is two searches per line where there was one. One
pass over the input saves work only when it replaces many short searches with
one *and nothing searches again* — which is the next point.

**The backward walk changes the language.** `tuned` finds the separator by
walking back from the end of the line. That is the last `;` of the line;
`until(";" | frame_end)` means the first. On `a;b;1.0` the grammar fails (the
temperature cannot start at `b`), the backward walk accepts a station named
`a;b`. No analysis makes the two agree: whether a second `;` is there is
exactly the question the forward scan answers, and asking it costs the scan.
A grammar that *wants* the last separator would have to say so — a construct
of its own, such as an `until` that keeps the last occurrence within the
frame — and that is a language decision, not an optimisation. It is not made
here; at 649 against 608 there is nothing to make it for.

**And it would have broken §1's promise.** The framed scan only works because
the fold hands its items something through the context, so the parser of
`NAME` would behave differently depending on whether a framed fold called it.
The answer would have been the same, but "the generated parser of a rule does
not depend on frames anywhere in the grammar" would no longer have been true,
for nothing.

None of it is in the tree; the variants are reproducible from this section.

## 10. How it was measured, and what the numbers say

Nikaia's `examples/1brc.nika` built as a project at `user-parallelism = "no"`,
`opt-level = 3`, against this crate by path (a `[patch]` in the project's
`.cargo/config.toml`), on 1 million rows from Nikaia's
`benches/brc` generator (413 stations, about a quarter of the names non-ASCII).
Every binary's output was compared byte for byte with the hand-written naive
one before anything was counted. Instructions are callgrind's total divided by
the row count, startup included (0.55 M, under one per row); branch figures
are callgrind's `--branch-sim`. Times are best of 25, the variants interleaved,
pinned to one core (`taskset -c 2`), over 8 million rows on a 4-core box.
Unpinned, best of 5, the same binaries moved by up to ±10 % from one round to
the next, so a time is read here only where it differs by more than that.
The baseline here is 676 per row; Nikaia's `benches/brc/README.md` quotes 587
for the same program, taken on another box and toolchain, and no figure here
is comparable with that one, only with each other.

**Instructions are not the cost here.** Every variant runs about four and a
half mispredicted branches per row — the hash table and the name length the
CPU cannot guess — and at fifteen to twenty cycles each those decide the clock.
§8 removed 68 instructions per row that were cheap, predictable ones; the
clock did not move. The variant in §9 that added 98 instructions added time,
because it added a second data-dependent search per row, and with it 1.6
mispredictions. What would move the
clock now is fewer unpredictable branches per row, not fewer instructions.

## Consequences

- A grammar that says `#[frame]` either compiles and can be cut soundly, or
  names the construct that stops it and what to write instead — for every
  construct the grammar itself contains. Where it reaches opaque Rust (§6),
  the guarantee is the author's, not the checker's.
- A rule that only lookahead (`peek(…)`, `not(…)`) reaches is not checked for
  the boundary: it consumes nothing, so it cannot carry the parser past one.
  The walk that resolves `frame_end` still follows lookahead, because
  `peek(frame_end)` names the boundary as much as consuming it does.
- The generated parser of a rule does not depend on frames anywhere in the
  grammar. §9 was rejected partly for that.
- `until(…)` accepts an alternation as its terminator, and one with up to
  three fixed alternatives (`until(";" | frame_end)`, `until("," | line_ending)`)
  is scanned in one pass; more than three take the position-by-position path.
- `frame_end` is a reserved built-in name.
- The `rayon` feature is the only optional dependency frames add, and nothing
  in the generated code changes when it is on.
- The syntax has room for the formats in §5 without a breaking change.
- A fixed-width field in a lexical rule costs one comparison per byte and one
  advance in the fast pass (§8). A grammar that wants that has nothing to
  write: `digit{1,2}` already says it.
- The remaining gap to `tuned` on this workload is not in the parser's
  instruction count (§10). A "last separator in the frame" construct is the
  one idea here that would change what is parsed; it is open, and needs a
  case that is not this one.
