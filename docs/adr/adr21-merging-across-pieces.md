# ADR 21: What a Merge Can See — Identity Across the Pieces of a `par_fold`

**Status:** Proposed. **Date:** 2026-09-08.
**Depends on:** ADR 16 (frames and `par_fold`), ADR 18 §3 (symbols are per
interner), ADR 19 §2 (`_pieces` shares the context; `_pieces_with` builds one
per piece), ADR 20 (`state T;`).

## Context

The same problem has now been hit from two directions, a week apart in reading
and an hour apart in fact.

* **Symbols are per interner.** Two pieces with interners of their own number
  their strings from zero, so two different words can share an id and one word
  can have two — silently (ADR 18 §3).
* **Slots are per state.** A grammar that declares `state Table;` and assigns
  slot numbers in a hand-written parser has exactly the same problem, one
  level up (ADR 20, `TODO.md` §6a).

ADR 19 §2 settled the first: `parse_<rule>_pieces` clones one context into
every piece, so the interner is shared by default and symbols mean the same
thing everywhere. The second is not settled, and this ADR is about what is
left.

The constraint that shapes every answer is in `rt::fold_pieces`:

```rust
M: Fn(T, T) -> T
```

**A merge sees two values and nothing else.** `rt::parse_piece` owns the
context it built, and drops it when the piece is done — so at merge time the
piece's table, its names, and its interner are already gone. A merge therefore
cannot resolve anything; it can only combine values that are already
comparable. Everything below follows from that.

## The options, and what each actually costs

Two of the four work today. The numbers are `benches/where.rs`, hot case, one
lookup: ~21 ns through the general interner, of which ~6 ns is the dashmap
shard lock, against ~7 ns for a bespoke direct-index table.

**1. Use symbols and let the shared interner do it.** After ADR 19 §2 this is
free and needs no thought: `intern(…)` yields ids that are comparable across
pieces because the pieces share the interner. ~21 ns per name, and correct by
default. *This is the answer for every grammar that is not chasing the last
factor of three, and it should be what the documentation leads with.*

**2. Share the table, as the interner is shared.** Works today, with nothing
added: `state Arc<Mutex<Table>>;` and a closure that clones the handle into
every piece (verified against the current tree). The slots are then global and
the merge combines by slot. The cost is the lock — the same trade the interner
already makes, and its ~6 ns of 21 is a fair estimate of it. *This is the
answer for a grammar that wants its own table and can pay a lock.*

**3. A table per piece, merged by name.** What a 1BRC-class solution actually
does: no synchronisation at all in the hot path, and one pass at the end that
combines by name — a cost per *distinct name*, not per row. This is the fastest
shape and **it is the one that cannot be written here**, because the merge does
not see the piece's table. Enabling it needs the piece's context to be
reachable once before it is dropped.

**4. Give the merge both contexts.** The general form of 3, and rejected as the
mechanism: `merge` is a closure written inside the grammar, in the DSL's
`par_fold(rule, init, step, merge)`, and widening it changes that syntax for
every user to serve one. A merge that needs a context is also a merge that can
read the interner and the diagnostics fields, which is more authority than the
job needs.

## Decision

**Lead with option 1, document option 2, and enable option 3 with the smallest
thing that does it — a per-piece `finish`.**

```rust
parse_<rule>_pieces_with(input, new_context, how)                    // today
parse_<rule>_pieces_finish(input, new_context, how, finish)          // proposed
//                                     finish: Fn(T, &ParseContext<S>) -> U
```

`finish` runs **inside the piece**, after its fold and before its context is
dropped, and maps the piece's accumulator into whatever is comparable across
pieces — typically `Vec<(name, value)>` keyed by name rather than by slot. The
merge then combines `U` values, which is the merge it already is. Three
properties make this the right size:

* It adds no DSL syntax. `par_fold`'s four arguments are unchanged; `finish`
  lives on the generated function, where the caller already chooses between
  `_pieces` and `_pieces_with`.
* It grants the minimum. A `finish` sees its own piece's context, not another
  piece's, and it runs once per piece rather than once per row.
* It is opt-in and orthogonal: a grammar that does not need it never sees it,
  and `rt::fold_pieces` grows one optional stage rather than changing shape.

**What decides whether it is worth building at all** is a measurement this ADR
does not have: option 2's lock against option 3's per-piece table, on a real
number of cores. This machine has four, which is enough to see a lock hurt but
not enough to be believed about where the curve goes. Until that is measured on
hardware worth measuring on, options 1 and 2 cover every case *correctly*, and
this decision is a plan rather than a schedule.

## Consequences

* Nothing changes for a grammar that uses symbols: ADR 19 §2 already made the
  shared interner the default, and `tests/shared_interner_test.rs` states it.
* Option 2 needs documenting rather than building — SYNTAX.md's `par_fold`
  section should show the shared-handle closure beside the fresh-state one,
  since the difference between them is exactly the difference between
  comparable and per-piece numbers.
* If `finish` is built, `TODO.md` §6a's open note — that a `par_fold` merges
  counts and not identities — closes, and `benches/interning.rs` gains the
  case that would justify it.
* The asymmetry that remains is worth naming: the interner is shared by
  *default* and the user state is not, because a state is the caller's data
  and a fresh one per piece is what most of them want. That is a deliberate
  difference, not an oversight, and `_pieces` versus `_pieces_with` is where it
  is expressed.
