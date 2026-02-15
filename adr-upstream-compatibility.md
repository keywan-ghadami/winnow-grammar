# ADR: Upstream syn-grammar Compatibility

## Status
Accepted

## Context
The upstream `syn-grammar` project has introduced significant changes, including:
1.  **Architecture Agnosticism**: A clear separation between the grammar model (`syn-grammar-model`) and the backend implementation.
2.  **Portable Primitives**: Standardized built-in parsers like `any`, `alpha1`, `digit1`, etc., to facilitate cross-backend consistency.
3.  **Portable Types**: Introduction of `Identifier`, `StringLiteral`, etc., to abstract away from `syn` specific types where possible.
4.  **Backend Trait**: A `Backend` trait that defines the capabilities and built-ins of a parser generator.

To maintain compatibility and leverage these improvements, `winnow-grammar` needs to align with these new interfaces.

## Decision
We will implement the following changes:

1.  **Implement `Backend` Trait**: `winnow-grammar-macro` will define a `WinnowBackend` struct implementing `syn_grammar_model::Backend`. This struct will register all supported built-in parsers and their return types.
2.  **Support New Primitives**: The code generation logic in `winnow-grammar-macro` will be updated to support the new portable primitives:
    - `any`: Maps to `winnow::token::any`.
    - `alpha1`: Maps to `winnow::ascii::alpha1`.
    - `digit1`: Maps to `winnow::ascii::digit1`.
3.  **Support Lookahead Operators**:
    - `peek(...)`: Maps to `winnow::combinator::peek`.
    - `not(...)`: Maps to `winnow::combinator::not`.
4.  **Update Codegen**: The `codegen` module will be refactored to handle the new `ModelPattern` variants (`Peek`, `Not`) introduced in `syn-grammar-model`.
5.  **Type Mappings**: We will map the portable built-in return types to their Rust equivalents in the `Backend` implementation (e.g., `alpha1` returns `String`).

## Consequences
-   **Compatibility**: `winnow-grammar` will be compatible with the latest grammar definitions and features from the upstream ecosystem.
-   **Feature Parity**: Users will be able to use the same rich set of primitives available in the `syn` backend.
-   **Maintainability**: Leveraging the shared `syn-grammar-model` reduces the maintenance burden for parsing and validation logic.
-   **Future Work**: We may need to further refine the mapping of "Portable Types" like `Identifier` to ensuring seamless interoperability with `winnow`'s output types. currently we map them to standard rust types or `syn-grammar` types.

## Implementation Details
-   **`winnow-grammar-macro/src/lib.rs`**: Implements `WinnowBackend` and registers built-ins.
-   **`winnow-grammar-macro/src/codegen/mod.rs`**: Handles `ModelPattern::Peek`, `ModelPattern::Not` and maps new built-in names to `winnow` combinators.
