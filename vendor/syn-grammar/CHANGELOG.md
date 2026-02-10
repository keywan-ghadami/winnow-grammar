# Changelog

All notable changes to this project will be documented in this file.

## [0.7.0]

### Added
- **Backend-Agnostic Model**: The `syn-grammar-model` crate now exposes `parse_grammar_with_builtins`. This allows backend authors (e.g., for `winnow-grammar`) to validate grammars against their own set of built-in rules, rather than being forced to use the default `syn` built-ins.
- **Backend Author Guide**: `EXTENDING.md` has been rewritten to focus on how to build custom parser generator backends using `syn-grammar` as the frontend DSL.

### Breaking Changes
- **Built-in Rule Resolution**: The precedence of built-in rules (like `ident`, `string`) has changed. They are no longer hardcoded keywords but are now provided as default implementations in `syn_grammar::builtins`.
    - **Impact**: If you define a rule named `ident` in your grammar, it will now *shadow* the built-in `ident` parser instead of being ignored. This fixes a long-standing limitation but may change behavior if you accidentally relied on the shadowing being ignored.
    - **Impact**: If you inherit from a grammar that defines `ident`, and you use `ident`, you might encounter ambiguity between the inherited rule and the built-in. Use explicit imports (e.g., `use super::Parent::ident;`) to resolve this.

### Added
- **Overridable Built-ins**: You can now replace standard token parsers by defining rules with the same name or importing functions with the same name. This enables backends (like `winnow-grammar`) to inject their own implementations for core types.
- **`syn_grammar::builtins` Module**: Exposes the default implementations of all built-in rules.

## [0.6.0]

### Added
- **`use super::*`**: The generated parser module now includes `use super::*;` by default, allowing parsers to seamlessly access other items defined in the parent module.
- **Use Statement Support**: Added support for standard Rust `use` statements within the grammar block (e.g., `use syn::Ident;`). These are passed through to the generated parser module.

## [0.5.0]

### Added
- **Span Binding Syntax**: Added support for the `name:parser @ span_var` syntax. This allows binding the result of a parser to `name` and its span to `span_var` simultaneously (e.g., `id:ident @ span`).

### Deprecated
- **Spanned Literal Parsers**: The `spanned_*_lit` built-in parsers (e.g., `spanned_int_lit`, `spanned_string_lit`) are deprecated. Use the standard literal parsers with the new span binding syntax instead (e.g., `lit_int @ span`).

## [0.4.0]

### Added
- **Token Recognition in Literals**: Enhanced parsing of string literals in the grammar to support multi-token sequences and complex combinations (e.g. `"?."`, `"@detached"`).
- **Pretty Error Printing**: The testing framework now pretty-prints `syn::Error` with source code context and underlining when assertions fail.
- **Outer Attributes**: Added support for parsing outer attributes (`#[...]`) via the `outer_attrs` built-in.
- **Span Binding**: Added support for capturing spans via `rule @ span_var` syntax.

### Improved
- **Error Spans**: Generated code now uses specific token spans instead of `Span::call_site()` where possible, resulting in more precise error highlighting in IDEs.

### Fixed
- **Documentation**: Fixed failing doctests in README, cleaned up examples, and clarified usage of brackets and delimiters.

### Internal
- **Testing**: Stabilized testing infrastructure.

## [0.3.0]

### Breaking Changes
- **Runtime Dependency**: Generated parsers now depend on the new `grammar-kit` crate (formerly `syn-kit`). Users must add `grammar-kit = "0.3.0"` to their `Cargo.toml`.
- **Renamed Built-in Parsers**:
  - `int_lit` has been renamed to **`integer`** (returns `i32`).
  - `string_lit` has been renamed to **`string`** (returns `String`).
  - This change distinguishes high-level value parsers from the low-level token parsers (`lit_int`, `lit_str`).

### Added
- **Attributes on Rules**: Rules can now be decorated with attributes, such as doc comments (`///`) or `#[cfg(...)]`.
- **Error Recovery**: Added `recover(rule, sync_token)` to handle syntax errors gracefully by skipping tokens until a synchronization point.
- **Rule Arguments**: Rules can now accept parameters (e.g., `rule value(arg: i32) -> ...`), allowing context to be passed down the parser chain.
- **Grammar Inheritance**: Grammars can inherit from other modules (e.g., `grammar MyGrammar : BaseGrammar`), enabling the use of external or manually written "custom parsers".
- **Testing Utilities**: Added `syn_grammar::testing` module with fluent assertions (`assert_success_is`, `assert_failure_contains`) to simplify writing tests for grammars.
- **Improved Error Reporting**: The parser now prioritizes "deep" errors (errors that occur after consuming tokens) over shallow errors.
- **New Built-in Parsers**:
  - `lit_int` -> `syn::LitInt`
  - `lit_char` -> `syn::LitChar`
  - `lit_bool` -> `syn::LitBool`
  - `lit_float` -> `syn::LitFloat`
  - `spanned_int_lit` -> `(i32, Span)`
  - `spanned_string_lit` -> `(String, Span)`
  - `spanned_float_lit` -> `(f64, Span)`
  - `spanned_bool_lit` -> `(bool, Span)`
  - `spanned_char_lit` -> `(char, Span)`

### Internal
- **Architecture**: Extracted runtime utilities (backtracking, error reporting, testing) into a separate `grammar-kit` crate.

## [0.2.0]

### Removed
- **`include_grammar!`**: Support for external grammar files (`.g`) has been removed.
  - **Reason**: Error reporting within external files was poor, making debugging difficult.
  - **Migration**: Please move your grammar definitions inline using the `grammar! { ... }` macro to benefit from full Rust compiler diagnostics and IDE support.

### Fixed
- **Generated Code**: Fixed usage of `syn` macros (`bracketed!`, `braced!`, `parenthesized!`) by removing incorrect error propagation (`?`).
- **Generated Code**: Changed rule variant generation to use a flat list of checks instead of `else if` chains, ensuring correct "first match wins" behavior and error fallthrough.

### Internal
- **Architecture**: Extracted grammar parsing, validation, and analysis into a separate `syn-grammar-model` crate. This enables the creation of alternative backends (e.g., `winnow`) in the future.
