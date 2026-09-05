# Changelog


## [0.1.0] - Unreleased

### Breaking Changes

- **Eigene Diagnose-Engine.** `parse_<regel>()` liefert `winnow_grammar::ParseError`
  statt `winnow::error::ContextError`. Der Meldungstext aendert sich: aus
  `invalid X` wird ``expected one of: `&`, identifier; found unexpected token `)` ``
  plus Regelstapel (`in typ`, `in arg`, `in item 1`). Auswahl nach Fortschritt,
  Prioritaet, Zusammenfassung - Vertrag in `docs/adr/adr15-diagnostics.md`,
  ein Test je Punkt in `tests/diagnostics.rs`.
  - **Migration**: Wer auf Meldungstexte prueft, passt sie an. Handgeschriebene
    Parser in einer Grammatik liefern `ErrMode<ParseError>`; `ParseError`
    implementiert `ParserError`, `AddContext<StrContext>` und
    `FromExternalError`, winnow-Kombinatoren funktionieren unveraendert.
- **`ParseContext`** hat zwei neue Felder, `furthest` und `regeln`. Ueber
  `Default` gebaut aendert sich nichts.

### Added

- **Varianten-Labels wirken.** `# "…"` wurde geparst und verworfen; jetzt wird
  der Name zur Erwartung, wenn die Alternative an ihrer Grenze scheitert.
- **Builtins nennen ihre Erwartung** (`identifier`, `integer literal`, …).
- **Listenelemente tragen ihren Index** (`in item 2`).
- **`ParseError::render(source)`** fuer Zeile/Spalte bei `parse_next`.

### Fixed

- Parser-Parameter in generischen Regeln (`list<T>(item)`) werden eingesetzt;
  fehlende Typparameter werden aus dem Argument abgeleitet.
- Ein WS-Zyklus (`WS -> comment -> WS` durch eine syntaktische Kommentarregel)
  wird zur Makro-Zeit gemeldet statt den Stack zu ueberlaufen.
- Aktionsbloecke duerfen Anweisungen enthalten (`-> { let x = …; x }`).

### Added
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
