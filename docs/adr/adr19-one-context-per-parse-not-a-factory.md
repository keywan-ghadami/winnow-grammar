# ADR 19: One Context, Cloned per Piece — Retiring the Context Factory

**Status:** Proposed, not implemented. **Date:** 2026-09-08.
**Depends on:** ADR 14 (the shared context), ADR 16 (frames and `par_fold`),
ADR 18 §3 and §4 (what the factory does and does not buy).

## Context

`parse_<rule>_pieces` takes a **context factory**:

```rust
pub fn parse_FILE_pieces<'a, S>(
    input: &'a str,
    new_context: impl Fn() -> ParseContext<S> + Sync,
    how: Parallelism,
) -> Result<Summary, ParseError>
```

`rt::parse_piece` calls it once per piece. That is right for most of what a
context holds: `furthest`, `rules`, `fold`, `diagnose` and `user_state` are
per-parse *mutable* state, and the pieces run concurrently through `&mut`, so
they cannot have one context between them.

It is wrong for the one field that ADR 14 exists for. The interner is meant to
be long-lived and shared, and sharing it through the factory is the caller's
job: clone the `Arc` inside the closure. ADR 18 §3 recorded what that has cost
in practice — **every call site in the repository, the documentation included,
passed `ParseContext::default`**, so every piece built an interner of its own
and symbols from two pieces were not comparable. Two different words could
share an id and one word could have two, with no error and no panic. The
mechanism the ADRs are built on had never once been exercised.

That is not a documentation failure to fix with more documentation. The
default was wrong: the safe thing was the thing you had to know to write.

## Decision

**`parse_<rule>_pieces` takes a context and clones it per piece. The factory
moves to a second entry point for the case that actually needs it.**

```rust
// The default: one context, cloned into every piece. The interner's `Arc`
// comes along, so symbols are comparable across pieces - without the caller
// knowing that this was a question.
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

`ParseContext` already derives `Clone`, and `S: Clone` is already a bound of
`rt::fold_pieces`, so the default costs nothing new in the type system. Cloning
a context per piece clones the `Arc` (cheap), the `Vec<&'static str>` of the
rule stack (empty at entry) and `S` (the caller's, and the caller chose the
default by calling it).

`rt::fold_pieces` keeps taking a factory — it is the lower layer, and a caller
with its own executor may want either. The generated `_pieces` builds the
trivial factory (`|| context.clone()`) for it.

### Why not the alternatives

**Keep the factory, add a debug assertion** that the pieces share an interner.
Rejected as the primary fix, and it does not stand alone: a grammar that never
interns is perfectly correct with an interner per piece, so a blanket assertion
is a false alarm. Making it precise means asking the analysis whether any rule
reachable from the `par_fold` interns — real work, to keep a default that is
still wrong by default.

**Tie `Symbol` to its interner in the type system** (a brand lifetime, or an
index into a context the type names). This is the fix that makes the mistake
impossible rather than unlikely. Rejected for now on cost: it appears in every
signature that carries a symbol, in user code as much as ours, and it buys
nothing else. Recorded here so it is not re-proposed as new.

**Leave it and document harder.** What ADR 18 §3 already did — the worked
example and the test are in the tree. They are worth having and they are not
enough: the trap is that the wrong thing is what a reader writes when they are
not thinking about interning at all.

## Consequences

* **Breaking**, for callers of `parse_<rule>_pieces`. The migration is
  mechanical: `ParseContext::<()>::default` becomes
  `&ParseContext::<()>::default()`, and a closure that was building a fresh
  state per piece moves to `_pieces_with`. Before 1.0, with a CHANGELOG that
  already carries breaking entries, this is the moment.
* A caller who wanted separate interners per piece — legitimate when symbols
  never leave their piece — says so by calling `_pieces_with`, which is the
  right way round: the surprising thing is the one you have to name.
* `tests/shared_interner_test.rs` changes shape: its first test becomes the
  plain call, and the second — the one pinning today's silent incomparability
  — becomes a test of `_pieces_with`, documenting that this is what asking for
  fresh contexts means.
* `SYNTAX.md` loses the warning it gained in ADR 18 §3, or rather turns it
  into a sentence about `_pieces_with`. The example gets shorter, which is the
  point.
* Nothing changes for `parse_<rule>()`, for `frames_<rule>()`, or for the
  meaning of any symbol.
