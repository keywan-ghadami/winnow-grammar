# Upstream Features Needed (syn-grammar)

The following features are desirable for `winnow-grammar` but require changes to the `syn-grammar` DSL (parser/AST) to be supported properly.

## User-defined Error Types
**Feature:** Support `#[error(MyError)]` or generic error types on the grammar.
**Current Status:** `winnow-grammar` hardcodes `winnow::error::ContextError`.
**Needed:** A way to specify the error type in the grammar definition.

## Combinator Wrappers (`expect`, `peek`, `not`, `map_err`)
**Feature:** Support for wrapping rules/patterns with combinators like `expect(rule, "msg")`, `peek(rule)`, `not(rule)`.
**Current Status:** `RuleCall` arguments must be literals (`Lit`). It is not possible to pass a `Rule` or `Pattern` as an argument (e.g. `peek(my_rule)` is invalid syntax).
**Needed:**
- Support for `Ident` or `Path` in rule arguments to reference other rules.
- OR Built-in patterns for `Peek`, `Not`, `Expect`/`Context`.

## User-defined Error Mapping
**Feature:** Allow mapping errors, e.g., `rule = pattern map_err(fn)`.
**Current Status:** No syntax for `map_err`.
**Needed:** Syntax support for error mapping on patterns or rules.

## Where Clauses & Generics
**Feature:** `rule<T> -> T where T: Display = ...`
**Current Status:** `syn-grammar` rules do not support generics or where clauses.
**Needed:** Update `Rule` AST to include generics and where clauses.

## Separated List
**Feature:** `separated_list(rule, separator)`
**Current Status:** Blocked by lack of rule-as-argument support (see Combinator Wrappers).
**Needed:** `RuleCall` support for rule references or a specific `SeparatedList` pattern.
