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
