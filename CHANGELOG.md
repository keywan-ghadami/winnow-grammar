# Changelog


## [0.1.0] - Unreleased

### Breaking Changes

- **Own diagnostics engine.** `parse_<rule>()` returns `winnow_grammar::ParseError`
  instead of `winnow::error::ContextError`. The message text changes: `invalid X`
  becomes ``expected one of: `&`, identifier; found unexpected token `)` `` plus
  the rule stack (`in typ`, `in arg`, `in item 1`). Selection by progress, then
  priority, then aggregation — the contract is `docs/adr/adr15-diagnostics.md`,
  one test per point in `tests/diagnostics.rs`.
  - **Migration**: code that checks message text adjusts it. Hand-written
    parsers plugged into a grammar return `ErrMode<ParseError>`; `ParseError`
    implements `ParserError`, `AddContext<StrContext>` and `FromExternalError`,
    so winnow combinators keep working unchanged.
- **`ParseContext`** has two new fields, `furthest` and `regeln`. Code that
  builds it through `Default` is unaffected.

### Fixed

- Parser parameters of generic rules (`list<T>(item)`) are substituted; missing
  type parameters are inferred from the argument.
- A whitespace cycle (`WS -> comment -> WS` through a syntactic comment rule)
  is reported at macro time instead of overflowing the stack.
- Action blocks may contain statements (`-> { let x = …; x }`).

### Added
- **Variant labels take effect.** `# "…"` used to be parsed and dropped; now the
  name becomes the expectation when the alternative fails at its boundary.
- **Built-ins name their expectation** (`identifier`, `integer literal`, …).
- **List items carry their index** (`in item 2`).
- **`ParseError::render(source)`** for line and column when using `parse_next`.
- **Inline Grammars**: Support for defining grammars directly in Rust code using `grammar!`.
- **EBNF Syntax**: Sequences, alternatives (`|`), optionals (`?`), repetitions (`*`, `+`), and groups (`(...)`).
- **Winnow Backend**: Generates efficient `winnow` parsers (`ModalResult<T>`).
- **Whitespace Handling**: Automatic whitespace skipping using `multispace0`.
- **Left Recursion**: Automatic compilation of direct left-recursive rules into loops.
- **Rule Arguments**: Support for passing arguments to rules.
- **Span Tracking**: Support for capturing spans with `@` syntax (using `LocatingSlice`).
- **Built-in Parsers**: `ident`, `integer`, `uint`, `string`, `char`, `hex_digit0`, `hex_digit1`, `oct_digit0`, `oct_digit1`, `binary_digit0`, `binary_digit1`, `float`, `space0`, `space1`, `line_ending`.
- **External Rules**: Support for calling custom or external `winnow` parsers.
- **Cut Operator**: Support for the cut operator `=>` to control backtracking.
- **Diagnostics**: Compile-time detection of indirect left recursion and unreachable alternatives (via `syn-grammar` 0.7).
- **Group Bindings**: Support for bindings on groups (e.g. `x:(a | b)`).
