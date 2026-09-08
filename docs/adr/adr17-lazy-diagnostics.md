# ADR 17: Lazy Diagnostics — Parse Fast, Explain on Failure

**Status:** Accepted. **Date:** 2026-09-08.
**Tests:** `tests/lazy_diagnostics_test.rs` (replay equals direct diagnosis,
the side-effect contract, the framed replay); the whole suite runs through
the replay path. Where the two disagree, this ADR wins.

## Context

ADR 15 gave every failure an explanation: the furthest position, every
expectation that competed there, what was actually found, the rule stack.
The engine that produces it is `ParseError`: a boxed struct with three
`String`-bearing fields, built by `from_stream` at every mismatch and
combined by `or`, `record` and `best`.

That engine ran on **every** parse, including the ones that succeed. A
literal that does not match inside an alternative that eventually does
allocates the box and the `found` word. Every `x?` that does not match,
every `x*` and `fold` that stops, clones the error it discards, pushes the
live rule stack onto it as `String`s and merges it into `furthest`. Every
rule call pushes its name onto the live stack. None of that is ever read
when the parse succeeds — and on a 13 GB measurement file parsed with
`par_fold`, nothing else is on the profile.

The way out is not to do the bookkeeping more cheaply but not to do it at
all — and to produce the explanation, when one is needed, by parsing again
with the engine of ADR 15 unchanged. That is sound because of a property
the generated code already has, which this ADR makes a promise:

> No control-flow decision reads the content of an error.

`labelled`, `expected`, `merge`, `record`, `best` and `finish` decide
*which* error is reported; `ErrMode::Cut` against `Backtrack` decides how
far an error propagates. Nothing decides whether a pattern *matches* by
looking at an error's offset, expectation or message. A parse with an empty
error type therefore accepts exactly the inputs a parse with `ParseError`
accepts, produces the same value, and fails at the same place.

## Decision

### 1. The generated parsers are generic over their error type

`parse_<rule>_inner` takes `E: rt::RtError<'a, S>` — winnow's own
`ParserError` and `AddContext<StrContext>` plus this crate's `Diagnostics`.
Two types fill it: `ParseError`, whose `Diagnostics` impl is the engine of
ADR 15 (moved out of `rt.rs` verbatim), and winnow's zero-sized
`EmptyError`, whose impl does nothing. The runtime helpers (`opt_recording`,
`repeat_recording`, `fold_recording`, `labelled`, `expected`, `fail`) call
the trait; with `EmptyError` every call is a no-op the compiler removes,
and `Diagnostics::RECORDING` lets the generated rule skip the live rule
stack as well.

The public `parse_<rule>()` keeps its signature and instantiates the rule
twice. This is winnow's own shape (`Parser<I, O, E>`), and it is the
smallest one: `ParseInput` does not mention the error type, so nothing
about the stream or the context changes for a caller.

### 2. Fast pass, then replay

`rt::entry` runs the `EmptyError` instantiation. If it accepts the whole
input, that is the answer and nothing else ran. If it fails — or leaves
input over, whose reason only the full engine can name — the input is reset
and parsed again with `ParseError`, and that error goes out through
`rt::finish` exactly as before. The messages are byte-identical to ADR 15's
because they are produced by ADR 15's code; the existing diagnostics tests
now run through the replay and prove it.

### 3. Frames narrow the replay to the failing item

A `par_fold` rule promises (ADR 16) that its items are independent of one
another and of any accumulated state — that is what lets pieces of the
input parse on separate cores. The same promise lets the replay skip what
the fast pass accepted. The fold in the body of a `par_fold` rule
(`rt::par_fold_recording`) writes, on the way out, how many items it
accepted and where it stopped (`ParseContext::fold`, a `FoldProgress`: two
words per failure, nothing per item). `rt::entry_framed` resets the input,
skips to that offset, and diagnoses from there: one item, then the error.
Item numbering continues from `fold.base`, so the message still says
`in item 4711`, not `item 1`; the offset is absolute because the replay
runs on the same stream. Errors recorded inside the accepted items lie
before this one and would lose on progress anyway.

This holds in the sequential `parse_<rule>()` and inside every piece of
`parse_<rule>_pieces()`, which parses each piece through the same entry.
With `Pieces(16)` on 13 GB a bad line costs one item in diagnose mode, not
800 MB and not 13 GB. Should the tail parse after all — an item that did
depend on what came before it, which no checked `par_fold` has — the whole
input is diagnosed instead, so the fallback is today's behaviour, never a
wrong answer.

A plain `fold` or `x*` over a `#[frame]` rule keeps the whole-input replay:
nothing there promises independence.

### 4. The caller chooses, on the context

`ParseContext::diagnose: Diagnose`:

| mode | fast pass | on failure | `user_state` |
|---|---|---|---|
| `Replay` (default) | yes | restore a clone of `user_state`, replay | an action ran once, as seen from outside |
| `ReplayInPlace` | yes | replay on the mutated context | an action ran twice |
| `Off` | yes | none — `ParseError::undiagnosed()` | as the fast pass left it |
| `Eager` | no | — (diagnosed directly) | as before ADR 17 |

`Off` is for a caller that needs the verdict and not the reason — a
validation step, a cheap first filter — and does not want to pay for a
second pass on the one input in a thousand that fails. Its error carries no
position; `render` prints its message alone, and `is_undiagnosed()` tells
it apart. `Eager` is what every parse did before this ADR and what a test
compares the replay against.

## The side-effect contract

The fast pass runs the grammar's actions, and the replay runs them again.
This is the one thing this ADR asks of a grammar that ADR 15 did not:

* **An action that mutates `user_state` must tolerate being replayed**
  after a failure. Under `Replay` the state is restored from a clone first,
  so the outside sees each action once — provided `S::clone` is a snapshot.
  A state that shares through `Arc` or `RefCell` is not restored by cloning;
  such a grammar chooses `ReplayInPlace` and lives with the double run, or
  `Eager`.
* **Interning is idempotent** and needs no care: a symbol interned twice is
  the same symbol.
* **A hand-written parser plugged into a grammar** still returns
  `ParseError` and is called in both passes; its error is converted through
  `Diagnostics::from_parse_error` (dropped in the fast pass). It does not
  get the fast path — it does what it does — but it does not break it.
* **Items of a `par_fold` rule are independent** — ADR 16 already required
  it for the pieces; the framed replay relies on it in the sequential parse
  too.

## Consequences

* A successful parse no longer allocates for diagnostics: no `Box`, no
  `found` word, no `item N` string, no rule-stack push. The diagnosing pass
  is unchanged, so nothing about the messages moves.
* A failing parse runs twice — the fast pass to the failure, then one item
  (framed) or the input (unframed) in diagnose mode. With `--features
  trace` both passes trace.
* Every grammar compiles twice per rule (two instantiations of `E`). Rules
  cannot use `E` as their own generic parameter, as they could not use `S`.
* `rt::fold_pieces` takes the two instantiations instead of one parser;
  `rt::RtError` (a trait) replaces the private alias of the same name.
* `ParseContext` has two new fields (`diagnose`, `fold`) and `ErrorCore`
  one (`undiagnosed`). Code that builds them through `Default` and reads
  fields is unaffected.
* ADR 14 still describes the context's interner field as `Arc<InternerContext>`;
  the `Arc` has since moved inside `InternerContext`. Noted, not fixed here.
