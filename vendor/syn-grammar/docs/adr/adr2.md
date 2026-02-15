# Architecture Decision Record (ADR) 2: Explicit Backend Contracts and Portable Types

## 1. Title

Explicit Backend Contracts and Portable Types for `syn-grammar`

## 2. Status

Accepted and Implemented. Supersedes implementation details of ADR 1.

## 3. Context

ADR 1 established the distinction between `PORTABLE_BUILTINS` and `SYN_SPECIFIC_BUILTINS`. However, it left the implementation details of how to enforce this contract and handle return types vague.

A critical issue identified is "Action Block Type Leakage". If `ident` returns `syn::Ident` in the default backend but `&str` in a `winnow` backend, a grammar using `ident` is not truly portable because the user's action code (e.g., `{ name.to_string() }`) might depend on the specific API of the return type.

To achieve true portability, the grammar must guarantee that portable primitives return **portable types** (or standard Rust types) regardless of the backend.

## 4. Decision

We implemented a comprehensive refactoring to formalize the backend contract and introduce portable types.

### Iteration 1: Formalize Backend Contract and Portable Types (Completed)

**Goal:** Replace implicit "magic string" built-ins with a formal, type-checked contract and introduce portable wrapper types for key primitives.

**Changes:**
1.  **`Backend` Trait:** Introduced `syn_grammar_model::Backend` trait.
    ```rust
    pub trait Backend {
        fn get_builtins() -> &'static [BuiltIn];
    }
    pub struct BuiltIn { name: &'static str, return_type: &'static str }
    ```
2.  **Portable Types:** Introduced `syn_grammar_model::model::types::{Identifier, StringLiteral}`.
    *   `Identifier`: Wraps `String` and `Span`. Implements `Display`, `ToTokens`.
    *   `StringLiteral`: Wraps `String` and `Span`. Implements `Display`, `ToTokens`.
3.  **`SynBackend` Implementation:**
    *   The default backend (`SynBackend`) now declares `ident` as returning `Identifier` and `string` as returning `StringLiteral`.
    *   **Breaking Change:** Users of `syn-grammar` now receive `Identifier` instead of `syn::Ident`.
4.  **`CommonBuiltins` Trait:**
    *   Refactored the runtime (`src/builtins.rs`) to use a `CommonBuiltins` trait.
    *   `parse_ident_impl` and other portable parsers are now generic over `T: CommonBuiltins`.

### Iteration 2: Introduce a Common `Spanned<T>` Wrapper (Completed)

**Goal:** Provide a portable way to access source location data (Spans) for any return type.

**Changes:**
1.  **`Spanned<T>` Struct:** Defined `pub struct Spanned<T> { pub value: T, pub span: Span }` in `syn-grammar-model`.
    *   Implements `ToTokens` (delegating to value) so it can be used in `quote!`.
2.  **Spanned Built-ins:** Updated `SynBackend` to expose spanned variants of all primitives:
    *   `spanned_i32` -> `Spanned<i32>`
    *   `spanned_char` -> `Spanned<char>`
    *   `spanned_bool` -> `Spanned<bool>`
    *   (and so on for all numeric types)
3.  **Runtime Support:** Implemented `parse_spanned_*_impl` functions using `Spanned<T>`.

### Iteration 3: Introduce Agnostic Core Data Types via New Built-ins (Subsumed)

**Goal:** Complete the set of portable types.

**Status:** This was largely subsumed by Iteration 1 and 2.
*   We decided to modify the return types of existing built-ins (`ident`, `string`) directly (breaking change) rather than introducing new ones (`sg_ident`), for a cleaner long-term architecture.
*   Standard types (`i32`, `char`, `bool`) are already portable.
*   `Spanned<T>` covers the need for location tracking on standard types.

## 5. Consequences

*   **Breaking Changes:** The default `syn` backend now returns `Identifier` for `ident`. Existing grammars expecting `syn::Ident` will fail to compile and must be updated. This is acceptable for the `0.x` release cycle to achieve clean architecture.
*   **Type Safety:** The validation step now checks if the user's grammar expects a type that matches the backend's declared return type for a built-in.
*   **True Portability:** A user writing `name: ident` can now write action code against `Identifier` (e.g., `name.text`) that will work identically on `syn` and `winnow` backends.
*   **Quote Integration:** Portable types `Identifier`, `StringLiteral`, and `Spanned<T>` implement `ToTokens`, allowing seamless use inside `quote! { ... }` macros, behaving like their `syn` counterparts but with portable guarantees.

## 6. Current Status

Refactoring is **Complete**. All tests pass.
