# Remaining High-Priority Tasks

This file tracks critical technical debt and optimization opportunities identified during development. These items represent features that are either partially implemented, stubbed out, or require significant refinement to meet production standards.

## 1. Optimize Cut Operator (`=>`) Implementation

*   **Current State:** The cut operator logic in `codegen/mod.rs` simply sets an `in_cut` boolean flag when it encounters a cut. Subsequent parsers in the sequence are then blindly wrapped in `::winnow::combinator::cut_err(...)`.
*   **The Issue:** This approach is somewhat naive. It might wrap too many things or not interact correctly with nested structures like `alt` or `delimited` in all edge cases. Specifically, `cut_err` prevents backtracking, which is the desired behavior, but indiscriminate wrapping can lead to confusing error messages or performance overhead if not scoped precisely. The logic for propagating the "cut" state through complex nested patterns (like groups or repetitions) needs verification.
*   **Goal:** Refine the `generate_sequence_steps` and `generate_step` logic to apply `cut_err` only at the exact necessary boundaries. Ensure that `cut` properly commits to the current alternative within an `alt` combinator without bleeding into unrelated parsing paths.

## 2. Robust Error Recovery (`recover`)

*   **Current State:** The `recover(rule, sync)` pattern is implemented in `codegen/mod.rs` using a combination of `alt`, `repeat`, and `peek`. It effectively says: "Try to parse `rule`. If it fails, consume characters one-by-one until `sync` is peekable, then return `None`."
*   **The Issue:**
    *   **Performance:** Consuming tokens one by one (`repeat(.., (not(peek(sync)), any))`) is inefficient (O(N^2) in worst case if `sync` is complex). It should use `winnow`'s optimized `take_until` or similar fast-scanning combinators if possible.
    *   **Correctness:** The current implementation assumes strict success/fail binary. Real-world recovery often needs to accumulate errors (diagnostics) rather than just returning `None`. The integration with `winnow`'s error reporting traits needs to be stronger so that the "skipped" bad input is reported as a specific error type to the user.
*   **Goal:** Replace the naive `repeat` loop with a more efficient scanning mechanism (e.g., `take_until` or `find_slice`). Consider extending the `recover` syntax or semantics to allow capturing the error for diagnostic reporting instead of just silently discarding it.

## 3. Map `winnow::stream::Location` to Proper Spans

*   **Current State:** The `@` binding syntax uses `.with_span()` which returns a `Range<usize>`. The code currently assumes the user will manually handle this `Range` or that it is sufficient.
*   **The Issue:** In many parser use cases (especially when using `LocatingSlice`), users want a rich `Span` object that might include line/column information, or they might be using a custom input type where `Range<usize>` isn't the natural span representation. The code comment explicitly states: *"This is where 'Map winnow::stream::Location to spans' task comes in... winnow-grammar currently just returns the Range as the 'span'."*
*   **Goal:** Make the span type configurable or smarter. If the input is `LocatingSlice`, we might want to return the slice itself or a custom `Span` struct. We need to verify if `winnow::stream::Location` is being fully utilized to provide rich location data (line/col) vs just raw byte offsets.

## 4. [Awaiting Integration] Detect Infinite Recursion Loops

*   **Upstream Status:** The `syn-grammar v0.7` dependency now includes a graph analysis pass that detects and reports cycles in the grammar rules.
*   **The Issue:** This analysis is performed in the upstream crate but is not yet explicitly invoked or tested within `winnow-grammar`. The compile-time errors or warnings it produces need to be verified to ensure they are presented to the user correctly.
*   **Goal:** Create test cases with indirect left-recursion and other complex recursive patterns to confirm that the upstream analysis correctly identifies them. Ensure that the error messages are clear and actionable for `winnow-grammar` users.

## 5. [Awaiting Integration] Add Diagnostics for Grammar Conflicts

*   **Upstream Status:** The `syn-grammar v0.7` dependency now includes FIRST/FOLLOW set analysis and can detect unreachable alternatives or ambiguous overlaps.
*   **The Issue:** Similar to recursion detection, this feature exists upstream but its results are not yet integrated into `winnow-grammar`'s diagnostic output.
*   **Goal:** Add test cases with ambiguous grammars (e.g., overlapping keywords, unreachable `alt` branches) to verify that the upstream diagnostics are triggered. Confirm that the warnings or errors are surfaced effectively during compilation of a `winnow-grammar` project.
