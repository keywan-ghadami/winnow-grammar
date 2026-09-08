# Remaining High-Priority Tasks

This file tracks critical technical debt and optimization opportunities identified during development. These items represent features that are either partially implemented, stubbed out, or require significant refinement to meet production standards.

## 1. Optimize Cut Operator (`=>`) Implementation

*   **Current State:** The cut operator logic in `codegen/mod.rs` simply sets an `in_cut` boolean flag when it encounters a cut. Subsequent parsers in the sequence are then blindly wrapped in `::winnow::combinator::cut_err(...)`.
*   **The Issue:** This approach is somewhat naive. It might wrap too many things or not interact correctly with nested structures like `alt` or `delimited` in all edge cases. Specifically, `cut_err` prevents backtracking, which is the desired behavior, but indiscriminate wrapping can lead to confusing error messages or performance overhead if not scoped precisely. The logic for propagating the "cut" state through complex nested patterns (like groups or repetitions) needs verification.
*   **Goal:** Refine the `generate_sequence_steps` and `generate_step` logic to apply `cut_err` only at the exact necessary boundaries. Ensure that `cut` properly commits to the current alternative within an `alt` combinator without bleeding into unrelated parsing paths.

## 2. Robust Error Recovery (`recover`)

*   **Current State:** `recover(rule, sync)` is `alt((rule.map(Some), (skip, sync).map(|_| None)))` in `codegen/expr.rs`. The skip is shared with `until` (`Codegen::generate_skip_to`): a literal, `line_ending` or `eof` sync is *scanned* for with `find_slice`/`memchr`; any other sync is still tried position by position.
*   **The Issue:**
    *   ~~**Performance:** Consuming tokens one by one is O(N²) in the worst case.~~ Done for fixed terminators. Still open: a sync that is a user rule whose body is a single literal takes the slow path; resolving through the rule would extend the scan to it.
    *   **Correctness:** The current implementation assumes strict success/fail binary. Real-world recovery often needs to accumulate errors (diagnostics) rather than just returning `None`. The integration with `winnow`'s error reporting traits needs to be stronger so that the "skipped" bad input is reported as a specific error type to the user.
*   **Goal:** Extend the `recover` syntax or semantics to allow capturing the error for diagnostic reporting instead of just silently discarding it.

## 3. Map `winnow::stream::Location` to Proper Spans

*   **Current State:** The `@` binding syntax uses `.with_span()` which returns a `Range<usize>`. The code currently assumes the user will manually handle this `Range` or that it is sufficient.
*   **The Issue:** In many parser use cases (especially when using `LocatingSlice`), users want a rich `Span` object that might include line/column information, or they might be using a custom input type where `Range<usize>` isn't the natural span representation. The code comment explicitly states: *"This is where 'Map winnow::stream::Location to spans' task comes in... winnow-grammar currently just returns the Range as the 'span'."*
*   **Goal:** Make the span type configurable or smarter. If the input is `LocatingSlice`, we might want to return the slice itself or a custom `Span` struct. We need to verify if `winnow::stream::Location` is being fully utilized to provide rich location data (line/col) vs just raw byte offsets.

## 4. Interning: where the time goes, and what is still open

`benches/interning.rs` (interning in place) and `benches/where.rs` (the same
call taken apart) exist so that this is decided on numbers. What they say
today, on one machine, hot case - eight distinct short words, every call a hit:

* One `intern_string` costs **~21 ns**: ~14 ns hashing and probing, ~6 ns the
  dashmap shard lock, ~1.5 ns lasso's own bookkeeping. Our wrapper around
  `ThreadedRodeo` costs nothing measurable.
* In a parse, that is **not a rounding error**: `parse/idents` runs ~40 ns per
  identifier end to end, so interning is more than half of what an identifier
  costs. The `parse/rows` case (`intern(until(";"))`, the 1BRC shape) is ~52 ns
  per row with one intern in it.
* String length barely matters: 38-character words cost ~25 ns against ~22 ns
  for seven-character ones. The per-call fixed cost dominates, not throughput.

### Swapping the hasher: measured, and **rejected** as first move

Three attempts, all slower than std's `RandomState`, all on `benches/where.rs`:

| hasher | ns per lookup |
|---|---|
| std `RandomState` (SipHash-1-3) | ~14 |
| FxHash-style (rotate, xor, multiply, no finalizer) | ~23 |
| the same with an xor-shift-multiply finalizer | ~25 |
| hand-written folded 128-bit multiply (wyhash-shaped) | ~30 |
| `ahash` | **~6** |

Two lessons. Speed here is **avalanche, not instruction count**: FxHash keeps
its entropy in the low bits while both the shard index and hashbrown's control
byte come from the top, so short keys collide and every lookup pays extra
string comparisons. And a hand-written hasher loses on the tail: a
variable-length `copy_from_slice` compiles to a real `memcpy` call, which costs
more than the multiplies save. Only a mature implementation (`ahash`, ~6 ns)
actually beats the default, and that is a dependency decision, not a
performance one - `ThreadedRodeo::with_hasher(ahash::RandomState::new())` is
the whole change if we ever want it.

### What should be checked next: a lookup cache in front of the interner

The hasher targets ~14 of the ~21 ns. A small cache targets **all of it**,
including the shard lock, on the case that dominates a real parse - the same
few names again and again.

The shape to try, and what to check about it:

* **Where.** A field on `ParseContext`, not on `InternerContext`.
  `intern_string` takes `&self` and the interner must stay `Sync`; the context
  is per-parse and reachable as `&mut` from generated code. `rt::fold_pieces`
  builds one context per piece, so a context-local cache is per-thread for
  free - no lock, no `thread_local!`.
* **Shape.** Direct-mapped, fixed size, one slot per bucket: on a miss or a
  tag mismatch, overwrite and fall through to the interner. As a *cache* it
  needs no collision handling and cannot be wrong - the interner remains the
  only authority on what a `Symbol` is.
* **Tag.** The first eight bytes as a `u64`, masked by length, is the cheap
  comparison - but only as a fast reject, verified against the real text (or
  by the interner fallback). `customer_id` and `customer_name` share their
  first eight bytes. Note that reading eight bytes near the end of the input
  is out of bounds: a `&'a str` from a caller carries no padding, so the tail
  needs its own path.
* **Size.** 32 KB is the wrong target: L1d is 32-48 KB *in total* and the
  parse also wants the input bytes, the stack and the output in there. Start
  at 512 entries (~8 KB) and measure 256/512/1024 - the bench has both a
  small-vocabulary case (`parse/idents`, high hit rate) and the case a cache
  cannot help (`interner/cold_1024_distinct_inserts`, every string new), which
  is where its overhead shows.
* **What to prove.** That the hit path is a few ns rather than ~21; that the
  miss path costs no more than ~2-3 ns over today; and that symbols are
  unchanged - the cache is an optimisation, not a semantic.

## 5. `fold.base` survives a parse (ADR 19 §1)

`fold_impl` numbers its items from `input.state.fold.base`, and
`rt::entry_framed` writes `base + seen` back when a `par_fold` fails. Nothing
resets it, so a context used for a second parse numbers from where the first
one stopped. Reproduced: the identical failing input reports `in item 4` with
a fresh context and `in item 7` with a reused one.

This hits exactly what ADR 14 advertises - one long-lived context across many
source files - and it blocks ADR 19 §2, which would clone the stale value into
every piece.

The fix is to reset the per-parse fields (`fold`, and defensively `furthest`
and `rules`) at the start of `rt::entry` and `rt::entry_framed`. Safe against
a nested entry point, because that composition does not exist: `rt::finish`
fails a parse with input left over, so an entry point called inside another
parse already fails with `expected end of input`. Wanted with it: a test that
parses twice through one context and gets the same message both times.

## 6. The high-end path: a bespoke interner, and why a grammar cannot have one

A 1BRC-class solution does not want a general interner. It wants the slot
number *itself*: one open-addressed table per thread, the first eight bytes as
the probe key, and the number handed back used directly as the index into the
accumulator array - no central map, no lock, no `resolve`, and no second
lookup at aggregation time. `benches/where.rs` measures that shape against
ours on the same workload: **~7 ns against ~21 ns**, and the ~7 ns includes
the trick that makes it: a name of eight bytes or fewer is entirely inside the
tag, length included, so a hit needs no string comparison at all.

Three things stand between a grammar and that, in the order they bite.

### 6a. The state type is not the grammar's (blocks everything else)

A bespoke table lives in `user_state`. Nothing the grammar can express
reaches it: a generated rule is generic over `S`, so `_state.user_state` has
type `S` in an action, **and a hand-written parser is no way round it** - it
is called from that same generic code, so naming a concrete state in its
signature is a type error (`expected Table, found type parameter S`).
Verified. Today `user_state` is the caller's: set before, read after,
untouchable in between.

What would open it: a grammar-level declaration of the state type - `state
MyState;` - so that the generated rules are concrete in `S`. **Designed in
`docs/adr/adr20-pinning-the-state-type.md`**, with the costs it is not free of:
composition between a pinned and a generic grammar becomes one-directional,
`parse_test` stops applying, and `user_state` becomes load-bearing under ADR
17's replay.
Then an action reaches `_state.user_state` and a hand-written parser can take
`&mut ParseInput<'a, MyState>`, which is the whole high-end path: the parser
computes a slot, the fold aggregates by it, `par_fold` gives each piece its
own table through `new_context`, and the merge combines them. Note that this
is also the case ADR 19 §2 calls an "escape hatch": for this class of
workload the *fresh* context per piece is the point, not the exception.

### 6b. `Symbol` hides the number it already has — **done**

`Symbol::index()` and `InternerContext::len()`/`is_empty()` are public, with
the contract written down: dense, zero-based, first-seen order, meaningful
only against the interner that made it, not to be persisted. A caller
aggregates into a plain `Vec` addressed by the index, with no second lookup -
`tests/interning_test.rs` has the worked case. What this does *not* change is
the cost of getting the number: ~21 ns per name, or whatever §4's cache makes
of it.

### 6c. The interner type is fixed

`ParseContext` names `InternerContext` concretely, so even a caller who has a
better interner cannot put it where `ident` and `intern` will find it. Making
it pluggable means a trait and a second type parameter on the context, which
lands in every generated signature - the same infection `S` already is, and
worth doing only together with 6a, if at all. Recorded so the three are
weighed as one question rather than three.
