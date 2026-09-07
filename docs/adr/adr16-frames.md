# ADR 16: Frames — Parsing an Input in Pieces

**Status:** Accepted. **Date:** 2026-09-07.
**Tests:** `tests/frames_test.rs` (the split, split + parse + merge against the
sequential parse, the driver), `tests/ui/frames.rs` (what the check rejects).
Where the two disagree, this ADR wins.

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

The first design on the branch did the checking, and additionally made the
generated parser conform: an `until(";")` reached from a `"\n"` frame was
silently generated as "until `;` or newline". The grammar did not say that. The
same rule text then parsed differently in two grammars, a rule shared between a
frame and something else changed behaviour for the something else, and the
code generator carried a per-rule mutable "current boundary" to do it. For a
feature whose whole point is that the grammar is believed only after it is
checked, an unwritten rewrite of the grammar is the wrong tool.

## Decision

### 1. A frame is a claim, and the claim is checked — never repaired

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

### 2. `par_fold` is `fold` plus a merge, and its parser skips nothing at its entry

`par_fold(rule, init, step, merge)` folds over a frame rule and must be the
whole body of its rule. Its generated entry point — alone among the rules —
skips no whitespace before or after the body. Every other rule's entry point
does, and that was a wrong total in the first design: the per-piece parser
*is* the entry point, so the skip ran once per piece instead of once per
input, and a frame beginning with a space lost the space in the piece that
happened to start there. The property `par_fold` promises is that pieces and
the sequential parse agree on every input, including the ones both reject; a
trailing blank line is therefore an error in both rather than tolerated in one.

### 3. Parallelism is the caller's to switch, size, or turn off

A `par_fold` rule generates three functions next to its parser:

- `frames_<rule>(input, n) -> Vec<Range<usize>>` — the cut;
- `merge_<rule>(a, b)` — the merge it was given;
- `parse_<rule>_pieces(input, new_context, how) -> Result<T, ParseError>` —
  the driver: cut, parse every piece with `parse_<rule>()`, merge in order, and
  report a failing piece's error at its offset in the whole input.

`how` is `rt::Parallelism`: `Off` (no cut, one parse — identical to
`parse_<rule>()`), `Pieces(n)`, or `Auto` (one per available core). Threads
come from the optional `rayon` cargo feature; without it the same driver runs
the pieces in sequence — the same cut and the same answer, which is what a
test uses to check the split without a thread pool. Thread count is rayon's
global pool's to configure; piece count is `Parallelism`'s. The first two
functions stay public so that a caller with its own executor needs nothing
from this crate but the cut and the merge.

`new_context` builds the `ParseContext` a piece parses with. That keeps ADR
14's shape: the caller owns the interner and shares it across pieces by
cloning an `Arc` in the closure.

### 4. UTF-8 is a guarantee of the split, not a restriction on it

The split works on bytes: `rt::frames_bytes(&[u8], &[u8], n)`, with
`rt::frames(&str, &str, n)` as its `&str` view. A valid UTF-8 boundary can only
match at a character boundary, so every range is one; that is a consequence
of the input being `&str` and costs nothing. There is no knob for it because
there is nothing to trade: a `&str` cannot be sliced mid-character. Should a
byte-oriented input type arrive, the primitive it needs is already the one in
use. The one restriction that *is* a judgement call — the conservative overlap
rule that rejects any literal sharing a character with a multi-character
boundary — is overridden by `unchecked`, not by a second rule.

### 5. The open flank: what the frame definition cannot say yet

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

### 6. Where the check runs

`frame::check` runs in the model's validator, where its errors are reported,
and again in the code generator, which needs its result (which rules are
frames and `par_fold`s) and has no other way to get it. The check is linear
in the grammar and pure; running it twice costs nothing observable. A
validator that returned its analysis would be the cleaner shape and is not
this ADR's concern.

## Consequences

- A grammar that says `#[frame]` either compiles and can be cut soundly, or
  names the construct that stops it and what to write instead.
- The generated parser of a rule does not depend on frames anywhere in the
  grammar.
- `until(…)` accepts an alternation as its terminator, and one with up to
  three fixed alternatives (`until(";" | frame_end)`, `until("," | line_ending)`)
  is scanned in one pass; more than three take the position-by-position path.
- `frame_end` is a reserved built-in name.
- The `rayon` feature is the only optional dependency this adds, and nothing in
  the generated code changes when it is on.
- The syntax has room for the formats in §5 without a breaking change.
