# Remaining High-Priority Tasks

This file tracks critical technical debt and optimization opportunities identified during development. These items represent features that are either partially implemented, stubbed out, or require significant refinement to meet production standards.

## 1. The cut operator — **verified, no change**

The concern was that setting a flag at the cut and wrapping every later step of
the sequence in `cut_err` might wrap too much or interact badly with groups,
delimiters and repetitions. `tests/cut_test.rs` answers it by behaviour rather
than by reading: a cut commits its alternative and no more; before it,
backtracking still works; inside a called rule it commits within that rule's
alternatives; as the element of a repetition it makes a half-matched element
fatal rather than ending the loop; and before a call to a rule with
alternatives it commits to the call while the alternatives still choose freely.
All six pass. Wrapping each later step is behaviourally the same as committing
the rest of the sequence, so there is nothing to scope more precisely.

The one thing the item asked for that is *not* the implementation: it wanted
the cut to commit "without bleeding into unrelated parsing paths". It does
bleed, by design - a cut is winnow's `cut_err`, so the failure is fatal and
propagates out of the rule that wrote it, and an alternative in a *calling*
rule is not tried either. That is what makes the reported error the committed
one's rather than a merge across everything that was attempted, and SYNTAX.md's
guidance elsewhere (write to a state after a cut, which no enclosing
alternative retries past) already relies on it. Changing it would mean catching
`Cut` at every rule boundary and turning it back into a backtrack, which would
defeat the operator. Documented instead: the Cut Operator section now says how
far it reaches and what that means for a rule meant to be called from
elsewhere.

## 2. Robust Error Recovery (`recover`)

*   **Current State:** `recover(rule, sync)` is `alt((rule.map(Some), (skip, sync).map(|_| None)))` in `codegen/expr.rs`. The skip is shared with `until` (`Codegen::generate_skip_to`): a literal, `line_ending` or `eof` sync is *scanned* for with `find_slice`/`memchr`; any other sync is still tried position by position.
*   **The Issue:**
    *   ~~**Performance:** Consuming tokens one by one is O(N²) in the worst case.~~ Done for fixed terminators. Still open, and now with numbers and a second half - see §8: a sync or terminator that is a user rule takes the slow path even when its body is a single literal, and under a `#[frame]` it is not slow but rejected.
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

### The lookup cache in front of the interner — **built**

`ParseContext` carries a direct-mapped table of 512 slots (8 KiB), and
`ParseContext::intern` - what `ident` and `intern(…)` call - answers from it
before it asks the interner. A miss falls through; the interner stays the only
authority on what a `Symbol` is, so nothing here can make one wrong.

Measured (`benches/interning.rs`, one machine):

| | interner | through the cache | |
|---|---|---|---|
| a hit, eight-byte words | 22.9 ns | **9.4 ns** | 2.4x |
| a hit, 38-byte words (verified) | 30.6 ns | **21.1 ns** | 1.5x |
| in a parse - `parse/idents/2000` | 162 µs | **97 µs** | **-43%** |
| in a parse - `parse/rows` (1BRC shape) | 258 µs | **190 µs** | **-26%** |

Two things the design turned on, both found by measuring rather than by
thinking:

* **The tail must not be copied.** Building the eight-byte tag of a short word
  with `copy_from_slice` compiles to a call to `memcpy`, which cost more than
  the whole rest of a lookup: 13.2 ns against 8.1 for the same cache with the
  bytes folded in a loop instead. The same trap ate two hand-written hashers
  in §4's first attempt.
* **The tag needs both ends.** With only the first eight bytes,
  `identifier_0001` and `identifier_0002` share a tag, so every miss between
  such words paid a resolve and a comparison before interning anyway - +30 ns
  on a miss. Mixing in the *last* eight bytes removed it.

What was hoped for and not reached: the miss path was to cost "no more than
2-3 ns over today". Across five runs it measured between nothing and ~20 ns
per miss on a ~150 ns insert - a few percent, with part of it the first touch
of a freshly allocated table, which a real parse pays once rather than per
batch. Left as it is: the case it costs is the one where the interner's own
insert dominates, and the case it pays for is every parse that sees a word
twice.

Not done, and deliberately: the cache is not consulted by
`InternerContext::intern_string`, which stays the direct path. An action that
calls `_state.interner.intern_string(…)` gets the interner; `_state.intern(…)`
gets the cache. Both return the same symbol, and SYNTAX.md says which is which.

## 5. `fold.base` survived a parse — **fixed**

`rt::entry` and `rt::entry_framed` now call `ParseContext::begin_parse`, which
clears the diagnostics engine's working space (`fold`, `furthest`, `rules`)
where a parse begins. Before that, a `par_fold` grammar parsed twice through
one context numbered the second parse's items from the first one's total, and
the offset grew with every failure: `in item 4`, then `7`, then `10`.

The scope was narrower than it first looked, and `tests/context_reuse_test.rs`
records all of it: only a `par_fold` rule (a plain `fold` runs untracked and
never read the base), only after a *failed* parse (a successful one leaves the
base at zero), and only in the diagnostic message - what a parse accepted and
what it returned never depended on it. The three tests that cover the defect
fail without the fix; that was checked by reverting it, not assumed.

ADR 19 §2 - `parse_<rule>_pieces` taking a context rather than a factory - was
blocked on this and is now open.

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

### 6a. The state type is the grammar's to declare — **done**

`state T;` in a grammar block makes the state reachable: an action writes
`_state.user()`, and a hand-written parser bounds itself with `StateOf<T>` and
calls `.state()`. The declaration is a bound rather than a substitution, so
rules stay generic, `parse_<rule>_pieces` keeps its signature, and one
composite state serves several grammars at once. ADR 20 has the design and
what each cost is met with; `tests/state_test.rs` has the high-end shape
end to end - a parser assigning slots out of the declared state under a
`#[frame]`/`par_fold` grammar.

What remains open around it: slot numbers are per state, exactly as symbols
are per interner, so a `par_fold` cut into pieces merges counts and not
identities unless the merge is keyed by name - and a merge cannot key by name,
because it sees two values and the piece's context is already dropped.
`docs/adr/adr21-merging-across-pieces.md` lays out the four answers and what
each costs. Two of them work today: symbols through the now-shared interner
(~21 ns, correct by default), or the table itself shared through the state
closure as `Arc<Mutex<_>>` (verified; costs a lock). The third - a table per
piece merged by name, which is what a 1BRC-class solution does - needs a
per-piece `finish` that sees its context before it is dropped, and is proposed
rather than scheduled: what decides it is a measurement on more cores than this
machine has.

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

## 7. The 1BRC temperature: where its time goes, and what is left

`benches/repetition.rs` takes `TENTHS` apart. Read differences, not absolutes -
every case pays the same stream construction and context clone. One machine,
`-12.3` parsed as a top-level rule:

| case | ns |
|---|---|
| `FLOOR` - an empty rule | 20.9 |
| `ONE_DIGIT` - `d:digit` | 23.9 |
| `TWO_DIGITS` - `a:digit b:digit`, no repetition | 26.0 |
| `BOUNDED_DISCARDED` - `digit{1,2}`, counted, not collected | 25.9 |
| `BOUNDED_BOUND` - `digit{1,2}`, collected into a `Vec` | 45.5 |
| `TENTHS` - the whole temperature | 60.6 |
| `tenths/by_hand` - one scan and a fold, written in Rust | 35.2 |

What that says:

* **The repetition loop is free.** Collecting nothing (25.9) costs what two
  separate `digit` parses cost (26.0). Checkpoints, the bound test and the
  loop itself do not show up.
* **One heap allocation for two `char`s costs ~20 ns** - the gap between
  collecting and counting the same two digits (45.5 vs 25.9).
* **The generated temperature is 1.7x the hand-written one** (60.6 vs 35.2),
  and ~20 of those 25 ns are that one allocation. Closing it would land within
  a few ns of hand-written **without** SIMD, SWAR or register arithmetic. The
  1BRC-shaped win is not clever numerics; it is not putting two characters on
  the heap.
* In a real `par_fold` the floor is not paid per record - the fold calls
  `parse_<rule>_inner`, not the public entry - so the allocation is a *larger*
  share of the per-record cost than the table suggests.

### Measured: what removing the allocation is worth

`benches/repetition.rs` carries a stand-in for a text-capture operator - the
digit run as a borrowed slice instead of a `Vec<char>`, which is winnow's
`.take()` under another name:

Three runs, because a single one said something that did not survive repeating:

| case | run 1 | run 2 | run 3 |
|---|---|---|---|
| `TENTHS` with `digit{1,2}`, a `Vec<char>` | 60.8 | 60.1 | 67.1 |
| `text(digit{1,2})`, folded in the action | 29.4 | 29.8 | 35.0 |
| `dec<i32>(digit{1,2})` | 34.4 | 35.2 | 38.6 |
| a `take_while` scan, folded in the action | 35.2 | 35.8 | 38.8 |
| written by hand, one scan and a fold | 33.8 | 34.1 | 39.1 |

(The third run is uniformly ~12% above the other two - machine drift. The
ordering is identical in all three, which is what the table is for.)

The allocation is ~23 ns and it is the whole story: not copying two characters
onto the heap lands on hand-written, with no SIMD, no SWAR and no register
arithmetic.

`text` halves the rule and lands below hand-written: the generated repetition
of `one_of` wrapped in `.take()` beats a `take_while` closure. `dec` costs
about 5 ns more than `text` plus the same fold in an action - `str::parse`
validates more than two bytes need - so it is bought for its two other
properties (the fold written once, and an overflow that fails the parse),
never for speed.

Both numbers were wrong until the benchmark measured the *generated* code
rather than a hand-written stand-in: the operators were generating their inner
pattern as if its values were wanted, collecting a `Vec<char>` that `.take()`
then dropped.

### What that leaves open

The allocation cannot be removed while the binding yields `Vec<char>`: the
type is the contract with the action, and `for d in whole` / `d.iter()` rely
on it. Two ways out, neither taken yet:

* **A text-capture operator** - "give me what was matched, not the parsed
  values", winnow's `.take()` under a DSL name, one entry in the fixed list in
  `parser.rs` the way ADR 18 added `intern`. It yields `&'a str`, costs
  nothing (it is a slice of the input), and works for any pattern. It also
  closes an inconsistency that is already in the language: `digit1` yields
  `&'a str` and `digit{1,2}` a `Vec<char>`, though both are a run of digits -
  which is why `intern(until(";"))` works today and `intern(digit{1,2})`
  cannot, a `Vec<char>` being no `AsRef<str>`. **This is where the ~23 ns are.**

* **`dec(p)`** - the run accumulated into an integer by the parser. Measured
  above: no time over the text operator plus a fold. Two arguments that are
  *not* about speed remain, and they are the ones to decide it on: an action
  no longer hand-rolls the same three-line fold at every numeric field, and
  the declared bound proves the accumulator cannot overflow, which a fold
  written in an action does not. Output type via the existing call generics:
  `dec<i32>(digit{1,2})`.
* **Inline storage for a small known bound** - `digit{1,2}` keeps its meaning
  but yields a stack-backed type. No new syntax, but it *is* a type change:
  actions that name `Vec<char>` break, and the binding type would differ
  between `{1,2}` and `{2,}`.

**Both are built.** `text(p)` and `dec<T>(p)` are in the language (SYNTAX.md,
`tests/text_test.rs`, `tests/dec_test.rs`). The inline-storage idea is dropped
with them: a borrowed slice beats a stack buffer and needs no container type
at all.

What is still open here is the default. `digit{1,2}` continues to yield
`Vec<char>`, which no grammar in this repository actually wants - the three
uses are a number, a count, and a `String`. Changing the default would need
codegen to know the element type (the builtin table knows `digit -> char`, the
model knows a rule's `return_type`, a rule parameter neither), and an output
type that depends on inference is a poor property for a DSL. Left as it is,
deliberately.

## 8. Scanning terminators: which ones, and the cliff between them — **done**

`until(…)` and `recover(…)` skip either by **scanning** - `find_slice`, which
is `memchr` with SIMD where the target has it - or by *trying* the terminator
at every position, one parser call per character. A rule of one's own never
qualified for the scan, even when its body was a single literal, which cost
this on 2000 rows of `name;digits\n`:

| | `until(";")` | `until(SEP)`, `SEP -> () = ";"` | |
|---|---|---|---|
| 8-character names | 29.6 µs | 81.0 µs | 2.7x |
| 40-character names | 36.3 µs | 263.3 µs | **7.3x** |

The scanned path barely moves with the field length; the tried path is linear
in it, so the factor grows with the input rather than sitting still.

**`analysis::literal_rules`** now resolves a rule that matches nothing but
literals - directly, through alternatives, or through a chain of such rules -
and **both** the code generator (§8a) and the frame check (§8b) read that one
map, so they cannot disagree about what a terminator is. Under a `#[frame]`,
`until(SEP | frame_end)` used to be *rejected* rather than slow, because the
check could not see the boundary alternative it was handed; it compiles now.
`tests/scan_terminator_test.rs` covers both, and asserts that the pieces of a
`par_fold` still agree with the whole.

The condition that keeps it correct, and that §8c wrote into SYNTAX.md: **only
a lexical rule is its literal.** A syntactic rule skips whitespace before its
elements, so `sep -> () = ";"` matches `  ;` and does not begin where its
literal does - `until(sep)` and `until(";")` stop in different places, and the
test pins both answers for the same input. Under a frame that whitespace could
also swallow the boundary.

### What is left, and it is a different question

A frame may now *end* in a rule that is its boundary as far as `is_terminator`
is concerned, but such a grammar still does not compile: the rule is also
walked as an interior rule of its own, where its literal is the boundary and is
flagged. Making it compile needs position-sensitive reachability - knowing that
`NL` is reached only as the terminator, which is the one position the interior
check already excludes. Pinned as a rejection in `tests/ui/frames.rs` so the
message says what it is.
