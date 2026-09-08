# ADR 20: `state MyState;` — Giving the Grammar Its Own State Type

**Status:** Proposed, not implemented. **Date:** 2026-09-08.
**Depends on:** ADR 14 (the shared context), ADR 17 (the replay and its
side-effect contract), ADR 18 §2 (`_state`), ADR 19 §2 (`_pieces_with`).
**Motivates:** `TODO.md` §6.

## Context

`ParseContext<S>` has carried a `user_state` since ADR 14, and every generated
rule is generic over `S`:

```rust
fn parse_row_inner<'a, S: std::fmt::Debug + Clone, E: RtError<'a, S>>(
    input: &mut ParseInput<'a, S>,
) -> winnow::Result<Row, ErrMode<E>>
```

That genericity is why **nothing the grammar can express can touch the user
state**. In an action, `_state.user_state` has type `S` and no operation
applies to it. A hand-written parser is not a way round it: it is called from
that same generic code, so it has to be generic too, and naming a concrete
state in its signature is a type error —

```text
error[E0308]: mismatched types
  expected `Table`, found type parameter `S`
```

— which this ADR's author found by writing one, having documented the
opposite the day before. So `user_state` is the caller's: set before the parse,
read after, untouchable in between. The only mutable per-parse state a grammar
reaches is the interner, because that field has a concrete type.

That is a gap on its own. It is also what blocks the one workload this crate
otherwise aims at. A 1BRC-class solution does not want a general interner: it
wants a table per thread whose slot number *is* the identity and indexes the
accumulator array directly. `benches/where.rs` measures the shape at **~7 ns
against our ~21 ns**, and the gap is structural, not tuning — no central map,
no lock, nothing to resolve, and no second lookup when aggregating. That table
belongs in `user_state`, so today it cannot be written.

`Symbol::index()` (`TODO.md` §6b, since implemented) closes part of this
without any of what follows: the built-in interner's number is dense, so a
caller can aggregate into a `Vec` addressed by it. What it does not do is make
the *lookup* cheaper or let a grammar bring its own.

## Decision

**A grammar may name its state type once, and its rules are then concrete in
it.**

```rust
grammar! {
    grammar Measurements {
        state Table;                       // <- the whole feature

        extern rule slot -> u32;           // now takes &mut ParseInput<'a, Table>

        #[frame(boundary = "\n")]
        pub ROW -> (u32, i32) = c:slot ";" t:i32 frame_end -> { (c, t) }
    }
}

fn slot<'a>(i: &mut ParseInput<'a, Table>) -> Result<u32, ParseError> { … }
```

The declaration sits inside the grammar block, in the shape `extern rule`
already established — a statement, not an attribute, so it is greppable and so
it does not collide with the rule that an attribute nothing reads is an error.

### What it changes in the generated code

`S` appears in about thirty places in the code generator, but the *parameter
lists* are built in three: the inner rule's generics, the outer entry point's,
and `parse_<rule>_pieces`. Pinning is one substitution — a `state_ty` on the
code generator that is either the parameter `S` (today's behaviour, when no
`state` is declared) or the named type, with the parameter dropped from those
three lists. Every other mention of `S` is a *use*, and becomes a use of the
named type.

The bounds do not move: `Clone + Debug` are what the runtime needs of a state,
so a pinned type must satisfy them, and the error when it does not should name
that rather than appearing inside expanded code.

### What it unlocks

* `_state.user_state` works in an action.
* A hand-written parser takes `&mut ParseInput<'a, Table>` and can do the work
  the DSL should not: a slot table, a symbol table with scopes, an arena.
* `par_fold` becomes the 1BRC architecture end to end: `_pieces_with` (ADR 19
  §2) gives each piece a fresh `Table`, the fold aggregates by slot, and the
  merge combines the tables. This is the case ADR 19 warned must not read as an
  exception; with `state` it is the ordinary entry point of that workload.

### It is additive

A grammar without `state` generates exactly what it generates today, generic
`S` included. Nothing existing changes, which is why this ADR does not need a
migration section.

## The costs, which are not zero

**1. Composition becomes one-directional.** A grammar generic in `S` can be
called with any state, a pinned one only with its own. So a pinned grammar may
call a generic one; a generic grammar cannot call a pinned one. That is sound
but it is a new way for two grammars to be incompatible, and it must fail with
a message that says so — an unresolved type parameter inside macro-expanded
code is not a diagnosis. The validator knows both grammars' declarations and
should check it.

**2. `parse_test` stops applying.** The test helper is fixed to
`ParseContext<()>` on purpose (so that tests need no turbofish). A pinned
grammar with a non-`()` state needs a sibling that takes the state, and the
existing helper should keep its signature rather than grow a parameter.

**3. `Default` is no longer a given.** `ParseContext::default()` requires
`S: Default`. A state that cannot be defaulted means the caller builds the
context by hand — fine for `parse_<rule>()`, but it is the second thing after
`parse_test` that assumes a state can be conjured.

**4. The replay clones the state — and it matters less than it looks.**
`rt::entry` and `rt::entry_framed` snapshot `user_state` before the fast pass
under `Diagnose::Replay`, so a pinned state is cloned once per parse, or once
per piece. Checked: the snapshot is taken *before* the parse, when a table
built by that parse is still empty, so the clone is cheap in the case this ADR
is for. It is not cheap for a caller who hands in a pre-filled state, and such
a grammar should choose `ReplayInPlace` or `Off` — ADR 17's side-effect
contract already says the second half of this, and gains the first.

**5. It makes `user_state` load-bearing.** Today it is inert, so nothing
depends on how it interacts with backtracking. Once a grammar writes to it
from an action or a hand-written parser, ADR 17's contract applies with teeth:
an alternative that writes and then backtracks has still written, and the
diagnosing replay runs the write again. A slot table tolerates that — assigning
a slot twice yields the same slot, exactly as interning does — but the ADR
should say so where a reader will look, because a counter does not.

## Alternatives

**A type parameter with a default** (`grammar M<S = Table>`) instead of a
statement. Rejected: the DSL has no generics on the grammar itself, and the
default would still leave every signature generic, which is what the type error
above is about.

**Make only the interner pluggable** (`TODO.md` §6c). It serves the
high-end interner and nothing else, and it costs a second type parameter on the
context — the same infection as `S` — for a strictly smaller result. If both
are ever wanted, `state` is the one that subsumes the other: a grammar that can
name its state can put whatever interner it likes in it.

**Leave it and let callers pre- and post-process.** What one does today: parse
to `&str` or `Symbol`, aggregate afterwards. It works, it costs a second pass
over the parsed values, and it is what `Symbol::index()` was just made to
serve. This ADR is not needed to make the crate useful; it is needed to make
the high-end shape *expressible*.
