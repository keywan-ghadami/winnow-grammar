# Architecture Decision Record (ADR): Portable and Backend-Specific Primitives

## 1. Title

A Unified Strategy for Portable and Backend-Specific Primitives

## 2. Status

Supersedes ADR of 2024-10-26. Accepted.

## 3. Context

`syn-grammar` is evolving into a universal parser frontend (`syn-grammar-model`) capable of driving multiple backends, including the token-oriented `syn` and character-oriented backends like `winnow`.

A key value proposition of a grammar DSL is the ability to use high-level, common primitives like `ident`, `integer`, and `string`. However, a fundamental impedance mismatch exists between backends:

-   **Token Streams (`syn`):** Operate on `proc_macro2::TokenStream`. The Rust lexer has already grouped characters into indivisible tokens (`Ident`, `LitInt`). Primitives like `ident` are trivial to parse (consume one token), but character-level primitives like `alpha` must be emulated by inspecting the token's content. Whitespace is lost.
-   **Character Streams (`winnow`):** Operate on `&str` or `&[u8]`. Character-level primitives like `alpha` are trivial (consume one character). High-level primitives like `ident` require more work, as the backend must parse a sequence of characters according to the language's rules.

We need a clear architectural contract that defines which primitives are considered portable across all backends and which are specific to the `syn` ecosystem.

## 4. Decision

We will establish two distinct categories of built-in primitives to make the portability contract explicit.

1.  **`PORTABLE_BUILTINS`**: This list will contain primitives that are **conceptually portable**, representing universal parsing concepts. All backends are expected to provide a meaningful implementation for them. A grammar that uses only these primitives is guaranteed to be portable.
    -   **High-Level Concepts**: `ident`, `integer`, `string`, `float`.
    -   **Low-Level Character Classes**: `alpha`, `digit`, `alphanumeric`, `hex_digit`, `eof`, `whitespace`.

2.  **`SYN_SPECIFIC_BUILTINS`**: This list will contain primitives that are **fundamentally tied to the `syn` crate's Abstract Syntax Tree (AST)**. They provide powerful shortcuts but knowingly lock a grammar into the `syn` ecosystem.
    -   **Examples**: `rust_type` (returns a `syn::Type`), `rust_block` (returns a `syn::Block`), `lit_str` (returns a `syn::LitStr`).

The responsibility for implementing these primitives lies with the backend. The abstraction focuses on the **conceptual contract**, not the implementation difficulty.

-   The `syn` backend implements `ident` by consuming a `syn::Ident` token. It implements `alpha` by consuming a `syn::Ident` and then running a filter on its string content.
-   A `winnow` backend implements `alpha` by consuming an alphabetic character. It implements `ident` by parsing a sequence of characters that constitute a valid identifier.

## 5. Alternatives Considered & Rejected

### Alternative A: Classify Primitives by Implementation Difficulty (Original ADR)

-   **Idea**: Only treat primitives that are simple for *all* backends (like character classes) as "universal." High-level primitives like `ident` would be considered `syn`-specific because they are "hard" for character-based backends to implement.
-   **Rejected Because**: This is the wrong abstraction. It prioritizes the backend's implementation convenience over the grammar author's experience. The main value of a grammar is working with high-level, common types. Forcing every `winnow-grammar` user to re-implement an `ident` parser is a critical failure of the DSL. The existence of `ident` parsers in the `winnow` ecosystem proves they are a universal expectation.

### Alternative B: Vendor-Specific Primitives (e.g., `winnow_ident`)

-   **Idea**: Allow each backend to inject its own proprietary primitives into the grammar.
-   **Rejected Because**: This leads to "Vendor Leaking" and destroys portability. A grammar written with `winnow_ident` cannot be used with the `syn` backend. The core DSL syntax must remain 100% portable.

### Alternative C: Force `syn` to Parse Character-by-Character

-   **Idea**: Make the `syn` backend behave exactly like a character-stream parser.
-   **Rejected Because**: This is technically impossible. `proc_macro2::TokenStream` does not permit partial consumption of tokens (e.g., taking just the first character of an `Ident`). Furthermore, all original whitespace is irrecoverably lost by the time the macro receives the tokens.

## 6. Consequences & Implementation Directives

-   **Clarity for Users**: Grammar authors now have a clear understanding of the portability trade-offs. If they stick to `PORTABLE_BUILTINS`, their grammar can target any backend.
-   **Burden on Backend Authors**: Authors of new, non-`syn` backends have a clear contract. They **must** implement all `PORTABLE_BUILTINS` to be compliant. This includes re-implementing lexing logic for high-level primitives like `ident` and `integer`. This is considered an acceptable trade-off for providing a high-quality, portable DSL.
-   **`syn-grammar-model`**: This crate will be the source of truth, exposing the `PORTABLE_BUILTINS` and `SYN_SPECIFIC_BUILTINS` constants for validation.
-   **`syn-grammar` (The `syn` backend) Implementation**:
    -   **`ident`, `integer`, etc.**: These map directly to consuming the corresponding `syn` token (`syn::Ident`, `syn::LitInt`).
    -   **`alpha`, `digit`, etc.**: These are implemented as **Token Filters**. The backend consumes the next logical token (e.g., a `syn::Ident` for `alpha`) and then applies a validation function to its content (e.g., `.chars().all(char::is_alphabetic)`).
    -   **`whitespace`**: This remains a zero-width assertion implemented via "span-gap detection" (checking if the end span of the previous token is adjacent to the start span of the next token).
