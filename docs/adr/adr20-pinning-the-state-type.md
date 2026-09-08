# ADR 20: `state MyState;` — Giving the Grammar Its Own State Type

**Status:** Proposed, not implemented. **Date:** 2026-09-08.
**Feasibility:** `tests/adr20_design_test.rs` — the three claims this design
turns on, compiled rather than argued.
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

### `state T;` declares a *requirement*, not an identity

The obvious reading — substitute `T` for `S` everywhere and drop the parameter
— is the wrong one, and this is the ADR's central choice. Substituting makes
two grammars with different states permanently incompatible: neither can call
the other, and no state can serve both. A **bound** costs the same and does
not:

```rust
// In the crate, once:
#[diagnostic::on_unimplemented(
    message = "the grammar declares `state {T}`, but this parse's state does not provide one",
    note = "give the parse a state of type `{T}`, or one that implements `StateOf<{T}>`"
)]
pub trait StateOf<T> { fn state(&mut self) -> &mut T; }
impl<T> StateOf<T> for T { fn state(&mut self) -> &mut T { self } }

// What `state Table;` generates:
fn parse_row_inner<'a, S: Debug + Clone + StateOf<Table>, E>(…)
```

The blanket impl means a state that simply *is* the table satisfies its own
bound with nothing to write. A composite state satisfies several, so two
grammars, each declaring its own `state`, run over one context:

```rust
struct App { a: TableA, b: TableB }
impl StateOf<TableA> for App { … }
impl StateOf<TableB> for App { … }
```

Both are compiled in `tests/adr20_design_test.rs`, including the coherence
question the blanket impl raises — `StateOf<App> for App` and
`StateOf<TableA> for App` are different instantiations and do not overlap.

`S` appears in about thirty places in the code generator, but the *parameter
lists* are built in three: the inner rule's generics, the outer entry point's,
and `parse_<rule>_pieces`. The change is one added bound in those three, plus
a `_user` binding injected beside `_state` in actions — `&mut Table`, so an
action writes `_user.slot(s)` and never spells the trait. `Clone + Debug` stay
where they are; the declared type inherits them.

Rules staying generic is not only about composition. It means
`parse_<rule>_pieces` and its `new_context` closure keep their signatures
exactly, `parse_<rule>()` keeps its shape, and a grammar that declares nothing
generates what it generates today — the feature adds a bound to some
signatures and changes nothing else.

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

## The costs, and what each one is met with

Every cost below was checked against a compiler rather than estimated, and
each is followed by what removes or bounds it. Two of the four turned out
smaller than the first draft of this ADR claimed.

**1. Two grammars with different states cannot compose.** *Met by the design
above.* With a bound instead of a substitution, a composite state satisfies
several requirements at once, and a grammar that wants to call another simply
declares the same `state T;` — a requirement composes where an identity does
not. What remains is that a grammar declaring nothing cannot call one that
declares something: the caller has to say what it needs. That is the same rule
as everywhere else in Rust, and the failure is a trait error, not an
unresolved type parameter inside expanded code. `#[diagnostic::on_unimplemented]`
puts it in the grammar's own words — verified on rustc 1.94:

```text
error[E0277]: the grammar declares `state Table`, but this parse's state does
              not provide one
   = note: give the parse a state of type `Table`, or one that implements
           `StateOf<Table>`
```

**2. `parse_test` stops applying.** *Met by a sibling, not a change.* The
helper is deliberately fixed to `ParseContext<()>` so tests need no turbofish;
a second trait with a different method — `parse_test_in(state, input)`, blanket
over every state — coexists with it, and the state argument determines the type
so neither call becomes ambiguous. Compiled, both calls, in the design test.

**3. `Default` stops being a given.** *Met by a constructor.* This one bites
immediately: `ParseContext { user_state, ..Default::default() }` does not
compile for a state that is not `Default`, which a pre-sized table need not be.
Naming every field instead requires nothing of `S`, so the fix is
`ParseContext::with_state(user_state)` wrapping exactly that. The design test
writes it out.

**4. `user_state` becomes load-bearing under ADR 17's replay.** *Half of this
was overstated, and the other half is real.*

The half that is handled: a failed parse under `Diagnose::Replay` — the
default — snapshots the state before the fast pass and restores it before
diagnosing, so an action that writes runs *once* as far as the state is
concerned. That is ADR 17's contract, and
`tests/lazy_diagnostics_test.rs::an_action_runs_once_under_replay_twice_in_place_once_when_eager_or_off`
already proves it. A counter is safe there; the first draft of this ADR implied
it was not.

The half that is real: **backtracking inside a successful parse undoes
nothing.** There is no snapshot at alternative granularity, so a branch that
writes and then loses has still written. Measured on the one writable
per-parse state that exists today — the interner — in
`tests/intern_test.rs::what_a_lost_branch_interned_stays_in_the_interner`.
`state` extends that exposure from interning to arbitrary data.

Three things bound it, in the order a grammar should reach for them:

* **Prefer idempotent writes.** Assigning a slot twice yields the same slot,
  exactly as interning does, so the high-end case this ADR exists for is
  unaffected by construction. This is the shape to design for, not a
  workaround.
* **Accumulate in the returned value, not in the state.** A count or a sum
  belongs in what a rule returns and what `fold`/`par_fold` combines — a path
  that backtracking handles correctly because a lost branch's value is
  discarded with it.
* **Write after a cut.** `=>` turns a later failure into `ErrMode::Cut`, which
  an enclosing `alt` does not catch, so no enclosing alternative can retry past
  a write that follows one.

**5. A pre-sized state is copied per piece.** *New in this review, small.*
`Diagnose::Replay` clones `user_state` at every entry — once per parse, once
per piece. For a state built empty and filled by the parse this is a copy of
nothing, which is the case this ADR is for. For a caller who hands in a table
pre-sized to 1024 buckets it is a real copy per piece; a batch job that wants
neither diagnostics nor the copy sets `Diagnose::Off`, which is the mode that
workload wants anyway.

## Alternatives

**Substituting the type instead of bounding it** — the first draft's reading of
`state`. Rejected above: it costs the same and makes two grammars with
different states permanently incompatible, with no state able to serve both.

**A type parameter with a default** (`grammar M<S = Table>`) instead of a
statement. Rejected: the DSL has no generics on the grammar itself, and a
default on the parameter does not give an action anything to call — the bound
is what does that.

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
