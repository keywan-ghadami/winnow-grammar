# Gemini3 Instructions

## Mandatory Checks

As final step, you must run the following commands to ensure code quality and correctness:

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
*   **Functional Changes:** You must update the `CHANGELOG.md` to document the changes, bug fixes, or new features.

## Version Control

1.  **Commit Frequency:**
    *   Execute `git commit` for **every single file change** with a descriptive commit message.
    *   Do not bunch multiple file changes into a single commit unless they are strictly atomic and dependent.

2.  **Dirty State Handling:**
    *   Before starting any new task, check for a dirty git state (uncommitted changes).
    *   If dirty state exists, analyze the changes to determine the intent and execute a `git commit` with an appropriate message *before* proceeding with new changes.

## Type of Changes

This file documents the types of changes made to the project to comply with `cargo clippy` and fix build errors.

### Lessons Learned

*   **`winnow-grammar-macro/src/codegen/mod.rs`:**
    *   Collapse nested `if-else` blocks to comply with `clippy::collapsible_else_if`.
    *   Remove unused import `syn::spanned::Spanned`.
*   **`tests/cron.rs`:**
    *   Simplify struct initialization to avoid `clippy::redundant_field_names` (e.g., `dom: dom` -> `dom`).
    *   Remove unnecessary `mut` references in `parse()` calls where `winnow` handles it, fixing `clippy::unnecessary_mut_passed` and `clippy::needless_borrow`.
*   **`tests/features.rs`:**
    *   Ensure grammar rules (e.g., `rule main`) have an action block `-> { c }` to prevent syntax errors and `undeclared type` errors.
