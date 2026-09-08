# ADR 19: The Context Holds Two Lifetimes — and `_pieces` Has to Choose

**Status:** Proposed; **blocked on the reset below**, which is a fix in its own
right. **Date:** 2026-09-08.
**Depends on:** ADR 14 (the shared context), ADR 16 (frames and `par_fold`),
ADR 17 (the diagnosing replay), ADR 18 §3 and §4.

## Context

`parse_<rule>_pieces` takes a **context factory**, called once per piece:

```rust
pub fn parse_FILE_pieces<'a, S>(
    input: &'a str,
    new_context: impl Fn() -> ParseContext<S> + Sync,
    how: Parallelism,
) -> Result<Summary, ParseError>
```

ADR 18 §3 recorded what that has cost: every call site in the repository, the
documentation included, passed `ParseContext::default`, so every piece built an
interner of its own and symbols from two pieces were not comparable — two
different words could share an id, one word could have two, with no error and
no panic. The sharing mechanism ADR 14 is built on had never been exercised.

The first draft of this ADR concluded the obvious thing: take a context instead
of a factory and clone it per piece, so the interner's `Arc` comes along by
default and the trap disappears. That draft was wrong, and this section says
why, because the reason is not visible from the signature.

**`ParseContext` holds two lifetimes in one struct.** One is long-lived and
meant to be shared — the interner, and `user_state` if the caller wants it.
The other is a *per-parse accumulator* owned by the diagnostics engine:
`furthest` (the best error a backtrack discarded), `rules` (the live rule
stack) and `fold` (where a `par_fold` stopped, which ADR 17's framed replay
reads to know where to restart). A clone carries both. So the question the
draft skipped is: what does a piece inherit that belongs to somebody else's
parse?

Two of the three are safe, and it is worth writing down why, because they are
what one worries about first:

* **`furthest` is cleared where it matters.** `rt::diagnose_from_here` sets it
  to `None` before every diagnosing pass, and the fast pass never writes it —
  `EmptyError`'s `Diagnostics::RECORDING` is `false` and its `record` is a
  no-op. A stale error cannot win a merge it is never part of.
* **`rules` is balanced.** The generated rule pushes its name, binds the
  result, pops, and *then* returns — so a failing rule pops too. After a parse
  the stack is empty.

**`fold` is not safe.** `fold_impl` reads `input.state.fold.base` to number its
items (`in item 3`), and `rt::entry_framed` reads it at the top and writes
`base + seen` back on the failure path. *Nothing ever resets it.* Reproduced on
the current tree — one context, the identical failing input parsed twice:

```text
in item 4      <- a fresh context
in item 7      <- the same context, after one failed parse
```

So a clone-per-piece API would hand every piece the caller's stale `base`, and
every piece's diagnostics would number its items from somebody else's total.
The fix for one silent wrongness would have introduced another.

And note where that bug already lives: **it needs no `par_fold` and no pieces.**
A caller who does what ADR 14 advertises — one long-lived context, many source
files — gets wrong item numbers from the second failing file onwards, today.

## Decision

### 1. First the reset, which is owed anyway

The per-parse fields belong to the parse, so a parse begins by owning them:
`rt::entry` and `rt::entry_framed` reset `fold` (and, defensively, `furthest`
and `rules`) at their start, before anything reads them.

This is safe against the one thing that would break it — an `entry` inside
another `entry`, which would reset its caller's accumulator — because that
composition does not exist: `rt::finish` fails a parse that has input left
over, so an entry point called inside another parse already fails with
`expected end of input` whatever it consumed. Verified, not assumed.

It is a fix in its own right, independent of everything below, and it should
land on its own with a test that parses twice through one context and gets the
same message both times.

### 2. Then, and only then, the signature

With the reset in place, what a piece inherits from a cloned context is no
longer a question — the parse resets it. So:

```rust
// The default: one context, cloned into every piece. The interner's `Arc`
// comes along, so symbols are comparable across pieces - without the caller
// having to know that this was a question.
pub fn parse_FILE_pieces<'a, S: Clone>(
    input: &'a str,
    context: &ParseContext<S>,
    how: Parallelism,
) -> Result<Summary, ParseError>

// The escape hatch: a fresh context per piece, for a `user_state` that must
// start empty rather than be copied.
pub fn parse_FILE_pieces_with<'a, S>(
    input: &'a str,
    new_context: impl Fn() -> ParseContext<S> + Sync,
    how: Parallelism,
) -> Result<Summary, ParseError>
```

`ParseContext` already derives `Clone` and `S: Clone` is already a bound of
`rt::fold_pieces`, so the default costs nothing in the type system. Per piece a
clone copies an `Arc`, an empty `Vec`, three words and the caller's `S`.

`rt::fold_pieces` keeps taking a factory: it is the lower layer, a caller with
its own executor may want either, and the generated `_pieces` gives it the
trivial `|| context.clone()`.

**The order is the decision.** Doing §2 before §1 would spread a live bug from
one context to every piece.

### 3. What is still not right, and is not fixed here

The reset makes the conflation *harmless*; it does not remove it.
`ParseContext` still bundles a handle meant to outlive many parses with an
accumulator that must not survive one, and `&ParseContext<S>` as a "template"
whose three diagnostic fields are ignored is an honest description of nothing.
The clean shape is two types — a shared handle the caller keeps, and a
per-parse context built from it.

Not proposed here, on cost: the interner is reached as `input.state.interner`
from generated code, from `_state.interner` in every action, and from
`ctx.interner` in user code and tests; splitting the struct renames all of it.
That is a large breaking change to buy tidiness once correctness is already
paid for by §1. Recorded so that it is weighed as a whole if the field ever
does move.

## Consequences

* §1 changes a message that is wrong today into the message a fresh context
  already produces. Nothing that was right changes.
* §2 is **breaking** for callers of `parse_<rule>_pieces`:
  `ParseContext::<()>::default` becomes `&ParseContext::<()>::default()`, and a
  closure building fresh state per piece moves to `_pieces_with`. Before 1.0,
  with a CHANGELOG that already carries breaking entries, this is the moment.
* A caller who wants an interner per piece — legitimate when symbols never
  leave their piece — says so by calling `_pieces_with`. The surprising thing
  becomes the one you have to name, which is the right way round. One caveat
  on the word "escape hatch": for a 1BRC-class grammar, a table per piece is
  not an exception but the whole design (`TODO.md` §6), so `_pieces_with` is
  that workload's ordinary entry point and should read like one, not like a
  footnote. It is not reachable today for a different reason — the state type
  is not the grammar's — but the signature should not be written as if that
  case were rare.
* `tests/shared_interner_test.rs` changes shape: its first test becomes the
  plain call; the second, which pins today's silent incomparability, becomes a
  test of `_pieces_with` and documents what asking for fresh contexts means.
* `SYNTAX.md` turns its `par_fold` warning into a sentence about
  `_pieces_with`, and the example gets shorter.
* Nothing changes for `parse_<rule>()`, for `frames_<rule>()`, or for the
  meaning of any symbol.

## Rejected

**A debug assertion that the pieces share an interner**, keeping the factory.
It cannot stand alone: a grammar that never interns is correct with an interner
per piece, so a blanket assertion is a false alarm, and making it precise means
asking the analysis whether any rule reachable from the `par_fold` interns —
real work to keep a default that is still wrong by default.

**Tying `Symbol` to its interner in the type system** (a brand lifetime, or an
index into a context the type names). The fix that makes the mistake
impossible rather than unlikely, and the expensive one: it appears in every
signature that carries a symbol, in user code as much as ours. Recorded so it
is not re-proposed as new.

**Documenting harder.** ADR 18 §3 already did that — the worked example and the
test are in the tree. Worth having, not enough: the trap is that the wrong
thing is what a reader writes when they are not thinking about interning at all.
