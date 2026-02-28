# Gemini3 Instructions

## Mandatory Checks

As final step, you must run the following command to ensure code quality and correctness:

1.  **Lint and Test:**
    ```bash
    cargo ctest
    ```
    Ensure there are no warnings, errors, and all tests pass.

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
    *   **Note Explanation:** When making code changes, explain the reasoning and what is being done in the commit message or as comments, not just at the end.

## Type of Changes

This file documents the types of changes made to the project to comply with `cargo clippy` and fix build errors.

### Lessons Learned

*   **Git Usage:**
    *   **Always** run `git add <file>` before `git commit`. The IDE environment does not automatically stage changes.
    *   Use `git status` to verify the state of the repository before and after operations.
    *   Commit messages should be descriptive and follow conventional commits (e.g., `feat:`, `fix:`, `docs:`).

*   **`winnow-grammar-macro/src/codegen/mod.rs`:**
    *   Collapse nested `if-else` blocks to comply with `clippy::collapsible_else_if`.
    *   Remove unused import `syn::spanned::Spanned`.
    *   **Cut Operator (`=>`):** Implemented by tracking `in_cut` state in sequence generation and wrapping parsers in `winnow::combinator::cut_err`.

*   **`tests/cron.rs`:**
    *   Simplify struct initialization to avoid `clippy::redundant_field_names`.
    *   Remove unnecessary `mut` references.
    *   **Type Inference:** Ensure explicit type annotations are used when necessary inside action blocks (e.g., `let tail: Vec<Field> = tail;`), as the macro expansion can sometimes confuse the compiler's type inference engine.

*   **`tests/features.rs`:**
    *   Ensure grammar rules have an action block `-> { c }` to prevent syntax errors.
