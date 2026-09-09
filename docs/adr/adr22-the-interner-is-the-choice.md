# ADR 22: Which Interner Is the Choice; Sharing, Threading and Merging Are Its Consequences

**Status:** Proposed, not implemented. **Date:** 2026-09-09.
**Supersedes:** ADR 20's rejection of a pluggable interner, which answered a
different question (below).
**Related:** ADR 14 (the shared context), ADR 19 (`_pieces` clones a context),
ADR 21 (what a merge can see), ADR 20 (`state T;`, and the bound trick this
proposal borrows).

## Context

Three questions have been argued separately in three ADRs, and they are one
question wearing three hats.

* **Share the interner or build one per piece?** ADR 19 §2 answered *share*, by
  making `parse_<rule>_pieces` clone one context into every piece.
* **Must it be thread-safe?** ADR 14 answered *yes*, by naming `ThreadedRodeo`.
* **Does a merge need to remap identities?** ADR 21 said it cannot, because a
  merge sees two values and the piece's context is already gone.

Each answer is right *for the interner this crate happens to name*, and wrong
for the next one. Set them side by side with what has been measured
(`benches/where.rs`, one machine, hot case):

| interner | shared across pieces | thread-safe | merge must remap | ns per lookup |
|---|---|---|---|---|
| `ThreadedRodeo` (today) | yes, by cloning an `Arc` | yes, a shard lock | no | ~21 |
| `Rodeo`, single-threaded | no | no | **yes** | ~16 |
| a direct-index slot table | no, one per piece | no | **yes** | **~7** |
| a pre-seeded or perfect-hash table | yes, read-only | trivially | no | not measured |

Reading down a column is what this ADR is about: **the sharing policy, the
threading requirement and the need for a merge are not independent decisions.
They are consequences of which interner is in the context.** ADR 21's merge
problem is not a separate problem; it is the shadow this one casts.

### The correction

ADR 20 rejected a pluggable interner in one paragraph: it "serves the high-end
interner and nothing else … for a strictly smaller result. If both are ever
wanted, `state` is the one that subsumes the other: a grammar that can name its
state can put whatever interner it likes in it."

The last clause is true and does not cover the case. A grammar can put a table
in its state and reach it **from a hand-written parser** - that is
`tests/state_test.rs`, and it is how the 1BRC shape became expressible. What it
cannot do is make `ident` and `intern(…)` use that table: those compile to
`ParseContext::intern`, which names `InternerContext` concretely. So a grammar
that wants a different interner must also stop using the two built-ins that
exist to intern. The rejection answered "can I have my own interner?" when the
question is "can the language's own interning operators use it?"

## Decision (proposed)

**`interner I;`, declared like `state T;` and implemented the same way: a
bound, not a type parameter.**

```rust
pub trait Interner {
    fn intern(&mut self, text: &str) -> Symbol;
    fn resolve(&self, symbol: Symbol) -> &str;
}
```

A grammar that declares one gets `S: InternerOf<I>` on its rules, exactly as
ADR 20 puts `S: StateOf<T>` there, and `ident` compiles to the trait call. A
grammar that declares nothing keeps today's `ParseContext::interner`, so this
is additive.

Three things make it affordable, and they are the same three that made ADR 20
affordable:

* **A bound, not a parameter.** A second type parameter on `ParseContext` would
  land in every generated signature. A bound rides on the `S` that is already
  there, and one state can satisfy several bounds - which is what lets a
  grammar have both a `state` and an `interner`.
* **`Symbol` stays the identity type.** The trait produces `Symbol`, so nothing
  in the language changes shape: a rule still returns `Symbol`, and
  `Symbol::index()` still names a row. This constrains an implementation to
  dense `u32` ids - which the interesting ones already are. A slot table's slot
  number *is* that id; it satisfies this trait as it stands.
* **Where the interner lives decides the policy.** It lives in the state, and
  ADR 19 already split that decision: `parse_<rule>_pieces` clones a context
  (share it), `parse_<rule>_pieces_with` builds one per piece (do not). Today
  that split governs the *user state* and the interner rides along by accident
  of being a field. With this, one mechanism governs both, and a caller who
  wants a per-piece interner says so the same way they say it for a table.

### What follows, rather than being decided separately

* **Thread-safety becomes a requirement of the caller's choice**, not of the
  crate's. A shared interner must be `Sync`; a per-piece one need not be, and
  a single-threaded implementation stops paying for a lock it cannot use -
  ~6 ns of the ~21.
* **A merge must remap exactly when the interner is per-piece**, which is ADR
  21's `finish` and nothing else. Neither ADR can be finished without the
  other: this one decides *when* a merge is needed, that one decides *how*.
* **`&mut self` is the general signature.** Today `intern_string` takes `&self`
  because `ThreadedRodeo` synchronises internally; a single-threaded interner
  cannot. `&mut` is the shape that admits both, and a shared implementation
  ignores it.

## Costs, and why this is proposed rather than built

* **The lookup cache does not obviously fit.** `ParseContext`'s cache keys on
  "the interner that made these symbols" and skips ~13 ns of a ~21 ns lookup.
  With a pluggable interner it either moves behind the trait - each
  implementation caching as it sees fit, and a slot table does not need one -
  or it stays and has to be told when the interner beneath it changed. The
  second is what exists; the first is probably right. Unresolved.
* **Three ADRs now touch this seam** (14, 19, 21), and each of them chose a
  default. Building this before ADR 21's measurement - the shared table's lock
  against a per-piece table on more cores than four - risks making the wrong
  one configurable and calling it finished.
* **The gain for the common case is zero.** A grammar parsing source code with
  a shared interner has exactly what it wants today; this is for the grammar
  that wants something else, and it should not slow the first one down. That is
  a bound's advantage over a parameter, and it should be measured rather than
  assumed.

**Recommendation: design it together with ADR 21, and build both once the
per-piece question has been measured on hardware worth measuring on.** What
this ADR settles now is the framing - that there is one choice here and not
three - and that ADR 20's rejection of it does not stand.
