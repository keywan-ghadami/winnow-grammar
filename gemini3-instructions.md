# Gemini3 Instructions

## Mandatory Checks

Before submitting any changes, you must run the following commands to ensure code quality and correctness:

1.  **Format Code:**
    ```bash
    cargo fmt
    ```

2.  **Lint Code:**
    ```bash
    cargo clippy --all-targets --all-features
    ```
    Ensure there are no warnings or errors.

3.  **Run Tests:**
    ```bash
    cargo test
    ```
    All tests must pass.

## Documentation Updates

If your changes involve:

*   **User Interface Changes:** You must update the `README.md` to reflect the new usage or behavior.
*   **Functional Changes:** You must update the `CHANGELOG.md` (or equivalent) to document the changes, bug fixes, or new features.

## Type of Changes

This file documents the types of changes made to the project to comply with `cargo clippy` and fix build errors.

### Fixes Applied

*   **`winnow-grammar-macro/src/codegen/mod.rs`:**
    *   Collapsed nested `if-else` blocks to comply with `clippy::collapsible_else_if`.
    *   Removed unused import `syn::spanned::Spanned`.
*   **`tests/cron.rs`:**
    *   Simplified struct initialization to avoid `clippy::redundant_field_names` (e.g., `dom: dom` -> `dom`).
    *   Removed unnecessary `mut` references in `parse()` calls where `winnow` handles it, fixing `clippy::unnecessary_mut_passed` and `clippy::needless_borrow`.
*   **`tests/features.rs`:**
    *   Fixed a syntax error in the `TestUse` grammar where the `rule main` was missing an action block `-> { c }`. This resolved the `unexpected end of input` and `undeclared type TestUse` errors.
