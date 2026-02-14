# Upstream Feature Requests

This document tracks feature requests for `syn-grammar` and `winnow-grammar-macro` that would simplify or improve the implementation of this project.

## 1. User-Defined Custom Error Types in `syn-grammar`

**Status:** Needed
**Priority:** High
**Context:** `syn-grammar` currently relies on `syn::Error` or simple error strings for error reporting.

### Use Case
We want to support rich, structured error reporting in the generated parsers, allowing users to define their own error types (enums or structs) that capture more context than a simple message and span.
For example:

```rust
enum MyParseError {
    UnexpectedToken { expected: String, found: String, span: Span },
    InvalidIdentifier(String, Span),
    // ...
}
```

The generated parser should be able to return `Result<T, MyParseError>` instead of `syn::Result<T>`.

### Why it's needed
- **Better Diagnostics:** `syn::Error` is great for macro expansion errors but might be limiting for full-blown parser error reporting where you want to inspect errors programmatically or format them in specific ways (e.g., with `ariadne` or `miette`).
- **Control:** Users need control over the error type to integrate with their application's existing error handling ecosystem.

### Current Limitations
- `syn-grammar` hardcodes `syn::Result` in many places (e.g., `parse_rule`, `attempt`, `recover`).
- The `rt::ParseContext` and error recovery mechanisms are tightly coupled to `syn::Error`.
- The macro expansion logic assumes `syn::Error` is the only error type.

### Proposed Solution / Feature Request
- Allow specifying a custom error type in the grammar definition, e.g., `grammar! { type Error = MyError; ... }`.
- The generated code should use this type in `Result<T, E>`.
- The custom error type should probably implement a specific trait (e.g., `From<syn::Error>`, `From<winnow::error::ErrMode<...>>`) so the generated code can convert internal errors into the user's error type.
- Alternatively, provide a way to map `syn::Error` to the custom error type at the boundary of each rule or the whole grammar.

## 2. Parameterized Rules (Generics)

**Status:** Needed
**Priority:** High
**Context:** Currently, rules in `syn-grammar` cannot take generic type parameters or lifetime parameters.

### Use Case
We want to define reusable rules that can parse different types of elements based on a generic parameter.
For example, a `separated_list` rule that takes an element parser as a parameter (conceptually similar to `winnow::combinator::separated`).

```rust
rule list<T>(element: impl Parser<T>, separator: impl Parser<()>) -> Vec<T> =
    // ... logic to parse T separated by separator ...
```

Or simpler, just passing arguments that affect parsing behavior.

### Why it's needed
- **Code Reuse:** Reduce duplication of similar rules (e.g., `ident_list`, `expr_list`, `type_list`).
- **Expressiveness:** Allow defining higher-order rules.

### Current Limitations
- `syn-grammar` parser for `Rule` struct does not support generic parameters `<...>`.
- The `RuleParameter` struct only supports simple name:type pairs, not generic constraints.
- The code generation logic doesn't know how to instantiate generic rules.

### Proposed Solution / Feature Request
- Extend the `Rule` syntax to support generic parameters: `rule name<T: Parser>(...) -> ...`.
- Update the code generation to include these generics in the generated function signature.
- This is a complex feature that might require significant changes to the macro and runtime.

## 3. Support for `separated_list` Pattern

**Status:** Needed
**Priority:** High
**Context:** Parsing lists of items separated by a delimiter (like commas) is a very common pattern (e.g., function arguments, array elements).

### Use Case
Currently, users have to manually implement the loop and separator handling, which is error-prone and verbose.
We want a built-in pattern or standard library rule for this.

```rust
// Ideally:
args: [Expr, ^","]*
// Or:
args: separated_list(Expr, ",")
```

### Why it's needed
- **Convenience:** Drastically reduces boilerplate for common grammar constructs.
- **Correctness:** Handles edge cases (trailing separators, empty lists) correctly.

### Current Limitations
- `syn-grammar` doesn't have a built-in `separated_list` pattern.
- The `Repeat` pattern (`*` or `+`) only supports simple repetition.

### Proposed Solution / Feature Request
- Add a specific syntax for separated lists, e.g., `[Elem, ^Sep]*` (using `^` or another marker for the separator).
- Or, implement it as a built-in rule in `syn-grammar::builtins` if we support parameterized rules (see #2).
- If built-in syntax is chosen, update `Pattern` enum and parser to support it.

## 4. `peek` Pattern for Lookahead

**Status:** Needed
**Priority:** High
**Context:** Sometimes we need to check if a token is present without consuming it, to decide which branch to take.

### Use Case
```rust
// If next is "async", parse async function, else parse normal function
rule item -> Item =
    { peek("async") -> async_fn }
    | { -> normal_fn }
```

### Why it's needed
- **Disambiguation:** Resolving LL(k) conflicts where one token of lookahead isn't enough or where we need to check a specific token before committing to a rule.

### Current Limitations
- No explicit `peek` construct. Users rely on the implicit lookahead of the generated parser, which is limited.

### Proposed Solution / Feature Request
- Add `peek(pattern)` syntax.
- Implementation: usage of `input.fork()` to check for match without advancing the original stream, or `winnow::combinator::peek`.

## 5. `not` Negative Lookahead Pattern

**Status:** Needed
**Priority:** High
**Context:** Ensuring a pattern does NOT match.

### Use Case
Useful for ensuring a keyword is not used as an identifier, or stopping repetition when a certain token appears.

```rust
ident: not(keyword) -> Ident
```

### Why it's needed
- **Reserved Words:** preventing keywords from being parsed as identifiers.
- **Ambiguity Resolution:** explicit exclusion of patterns.

### Current Limitations
- No support for negative lookahead.

### Proposed Solution / Feature Request
- Add `not(pattern)` or `!pattern` syntax.
- Implementation: `winnow::combinator::not`.
