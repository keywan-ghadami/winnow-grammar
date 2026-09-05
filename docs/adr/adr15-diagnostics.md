# ADR 15: Der Vertrag für Fehlermeldungen

**Status:** Accepted. **Datum:** 2026-09-01.
**Tests:** `tests/diagnostics.rs`, ein Test je Punkt. Bei Widerspruch gilt dieses ADR.

## Context

winnow-grammar soll das Frontend für Transpiler nach Rust sein. Dort ist die
Fehlermeldung das Produkt: wer eine fremde Sprache übersetzt, bekommt Fehler in
*ihr* und muss sie ohne Kenntnis der Grammatik verstehen.

Bis hierher kam der Fehlertyp von winnow (`ContextError`). Gemessen an
`fn f(a: );` mit `typ = ident | "&" ident`:

```
invalid typ
expected `&`
```

Vier strukturelle Lücken: `ContextError::or` liefert schlicht den *späteren*
Fehler, also gewann die letzte Alternative (`&`) und `ident` verschwand; nur ein
Label (das innerste), kein Regelstapel; kein Blick über ein erfolgreiches
Zurücksetzen hinweg (`x?`, `x*` verwarfen ihren Grund); und was tatsächlich
dastand, wurde nicht genannt. Varianten-Labels (`# "…"`) wurden geparst und vom
Codegenerator ignoriert.

syn-grammar hat dafür eine Engine (ADR 13 dort). Sie liess sich hier
**einfacher** bauen: Fortschritt ist im Text ein Byte-Offset, den
`LocatingSlice` umsonst liefert — der Cursor-Kunstgriff entfällt.

## Decision

Ein eigener Fehlertyp `winnow_grammar::ParseError` mit `offset`, `expected`,
`message`, `found`, `rule_stack`, `priority`. Er implementiert winnows
`ParserError`, sodass `alt` ihn durch `or` reicht — und `or` **ist** die
Fehlerauswahl:

1. **Fortschritt**: der Fehler mit dem grösseren Offset gewinnt — auch gegen ein
   `fail(..)`, das früher stand.
2. **Priorität** bei gleicher Stelle: `fail` (50) > Zusammenfassung (20) >
   Label (10) > Standard (0).
3. **Zusammenfassung** bei Gleichstand: die Erwartungen werden vereinigt.

Was `alt` nicht sieht — Fehler, die `x?` und `x*` bei einem *erfolgreichen*
Zurücksetzen verwerfen — merkt `ParseContext::furthest`; `rt::abschluss` hält
ihn am Ende gegen den zurückgegebenen Fehler. Ein gemerkter Fehler bekommt die
äusseren Regeln vom **lebenden** Regelstapel (`ParseContext::regeln`), weil er
den Rückweg nie geht.

## Der Vertrag

| # | Zusage | Beispiel |
|---|---|---|
| 1 | Alternativen an derselben Stelle werden zusammengefasst, und der Fund wird genannt | ``expected one of: `&`, identifier; found unexpected token `)` `` |
| 2 | Position als Zeile und Spalte, 1-basiert | `at line 2, column 8` |
| 3 | Regelstapel, innerste zuerst — auch für gemerkte Fehler | `in typ / in arg / in item 1 / in decl` |
| 4 | Ende der Eingabe wird als solches benannt | ``unexpected end of input, expected `;` `` |
| 5 | Resteingabe nennt den Grund, nicht nur "expected end of input" | ``expected `;`; found unexpected token `extra` `` |
| 6 | Eine benannte Alternative (`# "…"`) steuert an ihrer Grenze ihren Namen bei | `expected one of: number, string` |
| 7 | … aber nur ohne Fortschritt; sonst bleibt ihre eigene Meldung | ``expected `"` `` |
| 8 | `fail("…")` gewinnt bei gleicher Stelle | `custom failure here` |
| 9 | Fortschritt schlägt `fail` | ``expected `b` `` statt der `fail`-Meldung |
| 10 | Listenelemente tragen ihren Index | `in item 2` |
| 11 | `Display` ohne Position, `render(source)` mit | winnows `Parser::parse` stellt die Position selbst voran |
| 12 | Der Fehler ist ein Wert mit Feldern | `e.expected`, `e.found`, `e.rule_stack`, `e.offset` |

Builtins bekommen eine Erwartung (`identifier`, `integer literal`, …), weil
winnows Primitiven nur die Stelle melden.

## Consequences

* **Breaking:** `parse_<regel>()` liefert `winnow_grammar::ParseError` statt
  `ContextError`. Der Meldungstext ändert sich: aus `invalid X` wird
  ``expected …; found unexpected token `…` `` plus `in X`. Handgeschriebene
  Parser, die in eine Grammatik eingehängt werden, müssen `ErrMode<ParseError>`
  liefern; `ParseError` implementiert `FromExternalError` für `parse_to()` und
  `AddContext<StrContext>`, sodass winnow-Kombinatoren unverändert
  funktionieren.
* `ParseContext` hat zwei neue Felder (`furthest`, `regeln`). Wer ihn per
  `Default` baut, merkt nichts.
* Was noch fehlt, gegenüber syn-grammar: `recover(..)` meldet den
  übersprungenen Fehler nicht (er wird verworfen, nicht gemerkt), und `until`
  hat keine Erwartung. Beides sind Ergänzungen, keine Umbauten.
