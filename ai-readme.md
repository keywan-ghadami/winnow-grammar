# winnow-grammar Architecture Notes

## structure
- **Front:** `syn-grammar-model` (AST).
- **Back:** `winnow-grammar` -> `winnow` parsers.
- **Macro:** `winnow-grammar-macros/src/codegen/mod.rs`.

## codegen logic
- **Modes:** 
  - `Syntactic` (`!is_lexical`): Implicit `ws` between steps.
  - `Lexical` (`is_lexical`): Strict, no implicit `ws`.
- **Whitespace (`ws`):**
  - Defaults to `multispace0`. Can be overridden by rule `ws`.
  - **Wrapper:** Public rules get `parse_{name}` wrapper: `ws?` -> `inner` -> `ws?` -> `eof`.
  - **Sequences:** `step1` -> `ws` -> `step2`.
  - **Delimiters:** `open` -> `ws` -> `inner` -> `ws` -> `close`.


## spans & offsets
- **Wrapper effect:** Wrapper consumes leading `ws`. Inner rule starts *after* `ws`.
- **Spans:** `t:term @ s` -> `s` excludes leading whitespace consumed by wrapper.
- **EOF:** Syntactic `item eof` -> `item` -> `ws` -> `eof`. Input `item <space>` succeeds.

## primitives
- `u32`, `i32` etc map to `winnow::ascii::dec_uint/int`.
- `ident`, `string`, `char` have custom winnow implementations in codegen.

## Wichtige Hinweise für KI-Agenten

1.  **Antworte auf Fragen, handle nicht voreilig:** Wenn eine Ja/Nein-Frage gestellt wird, gib eine direkte Ja- oder Nein-Antwort. Interpretiere die Frage nicht als implizite Aufforderung, eine Aktion auszuführen.
2.  **Erfinde keine Syntax:** Das `grammar!`-Makro hat eine sehr spezifische, nicht-generische Syntax. Versuche nicht, Generics (z.B. `<'a>`), `where`-Klauseln oder andere Syntax hinzuzufügen, die nicht explizit von der Definition des Makros unterstützt wird. Das Makro ist kein Standard-Rust-Makro und hat seine eigene domänenspezifische Sprache (DSL).
3.  **Analysiere, bevor du handelst:** Bevor du versuchst, ein Problem zu beheben, analysiere den vorhandenen Code, insbesondere die Makros und Testmuster, um den richtigen Ansatz zu verstehen. Mache keine Annahmen darüber, wie die Dinge funktionieren *sollten*.
4.  **Bestätigung einholen:** Bevor du komplexe Aktionen oder Änderungen am Code vornimmst, präsentiere den Plan dem Benutzer zur Genehmigung.
5.  **Benutze `cargo expand`, um die Makro-DSL zu verstehen:** Das `grammar!`-Makro ist eine domänenspezifische Sprache (DSL). Um ihre korrekte Syntax für komplexere Anwendungsfälle (z.B. manuelle Tests ohne das `test_case!`-Makro) zu verstehen, **rate nicht**. Verwende stattdessen den Befehl `cargo expand --test <test_name>` (z.B. `cargo expand --test char_test`) für eine funktionierende Testdatei. Analysiere den expandierten Code, um die exakte Struktur zu verstehen, die das Makro erzeugt, und wende dieses Wissen an, um neuen Code zu schreiben.
