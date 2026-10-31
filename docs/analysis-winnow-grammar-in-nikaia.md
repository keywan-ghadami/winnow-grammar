# winnow-grammar in Nikaia schreiben? – Fragenkatalog (Schritt 1)

**Leitfrage:** Welche Vorteile hätten wir, wenn wir winnow-grammar in der
Programmiersprache Nikaia schreiben?

**Stand:** 2026-10-31, Schritt 1 von mehreren. Dieses Dokument enthält noch
keine Antworten. Es sammelt (a) die Fakten, die wir für die Antwort brauchen,
und (b) die Fragen, die sich daraus ergeben: zuerst aus Sicht der Sprache
Nikaia, dann aus Sicht von winnow-grammar. Gewichtung und Antworten folgen,
nachdem wir das weitere Vorgehen besprochen haben.

Quellen: dieses Repository (Branch `ccr-f825bc66-x6i9e4`) und
`Nikaia-Foundation/Nikaia`, Version 0.0.1317. Pfade beginnen mit `wg:`
(winnow-grammar) oder `nk:` (Nikaia).

---

## 0. Ausgangslage

### 0.1 Wie die beiden Projekte heute zusammenhängen

* **Nikaia ist der Hauptnutzer von winnow-grammar.** Der Compiler-Parser ist
  ein Rust-`grammar!`-Block (`nk:crates/nikaia/src/parser/mod.rs`, ab Zeile
  953, 358 Regeln). Nikaia hat außerdem ein eigenes Sprachkonstrukt
  `grammar Name { rule … }` (Part II Kap. 10, ADR-296, ADR-339,
  ADR-grammar-1). Es wird **syntaktisch** zu `grammar! { … }` abgesenkt
  (ADR-296 D17, D23). Danach erzeugt das winnow-grammar-Proc-Macro den
  winnow-Code.
* Die Kette ist also `.nika` → Nikaia-Lowering → Rust mit `grammar!` →
  Proc-Macro (`syn`/`quote`) → winnow-Code + `rt`-Laufzeit.
* Nikaia-Grammatiken gibt es in `tools/rust.nika`, `tools/ledger.nika`,
  `runtime/http1.nika`, `packages/yaml` (1.303 Zeilen) und
  `examples/1brc.nika`.
* **Viele Features von winnow-grammar kamen aus Nikaia.** 24 von 92 Commits
  nennen Nikaia. Byte-Eingabe (ADR 25) kam aus Nikaia#422, Frames und
  `par_fold` (ADR 24) wurden an Nikaias 1BRC gemessen. Dazu kommen
  `forbid(unsafe_code)` im generierten Code (ADR 24 §12a) und die Ausrichtung
  der Fehlermeldungen an Nikaias Fehlerkorpus (ADR 15/17).
* **Die Trennung hat bekannte Reibung.** In
  `nk:docs/history/staging-candidates.md` §2.1 steht: „Every parser decision
  is therefore made one level down … The obstacle is location, not
  difficulty“. Cargo fingerprintet den Git-Commit, eine lokal geänderte Kopie
  wirkt also nicht. Gefundene Defekte gehen über
  `nk:docs/upstream/winnow-grammar-findings.md` an dieses Repository.
* **Die Git-Abhängigkeit blockiert crates.io.** Laut ADR-336 nimmt crates.io
  keine `git`-Abhängigkeit an.
* **Bisher ist kein Umbau geplant.** Keine ADR und kein Plan schlägt vor,
  winnow-grammar in Nikaia zu schreiben. Die einzige Spur ist die
  Roadmap-Zeile zu #928: „in the parser generator, not yet written by
  Nikaia“. ADR-294 D2/D5 erlaubt ausdrücklich, dass `winnow` als
  „Adapter zu einem Crate“ in Rust bleibt.

### 0.2 winnow-grammar in Zahlen

| Teil | Zeilen | Aufgabe |
| :--- | ---: | :--- |
| `winnow-grammar-model` | 6.601 | Grammatik-DSL parsen (über `syn`), Validierung, FIRST-Mengen, Linksrekursion, Frames, Streams |
| `winnow-grammar-macros` | 4.937 | Codegen mit `quote!`, Proc-Macro `grammar!` |
| Laufzeit `src/` | 6.944 | `ParseContext`, Fehler-Ranking, Interner (lasso), Scans (memchr/SWAR), Stream-Leser, 26 `unsafe`-Blöcke |
| Tests | 76 Dateien, 397 `#[test]`, 13 trybuild-UI-Fälle | |
| ADRs | 14 (`docs/adr/adr14…27`) | |

### 0.3 Erste Klärung: Was heißt „in Nikaia schreiben“?

Die Antwort auf die Leitfrage hängt stark davon ab, welcher Teil gemeint
ist. **Nikaia hat absichtlich keine Makros** (Part II 10.3: „There is no
macro system“, ADR-298 D6, ADR-304). Ein Nikaia-Programm kann also kein
Rust-Proc-Macro sein. Es gibt mehrere denkbare Varianten:

| Variante | Inhalt | Wer nutzt das Ergebnis? |
| :--- | :--- | :--- |
| **A** Generator im Nikaia-Compiler | Nikaias `grammar`-Lowering erzeugt selbst winnow-Code statt `grammar!`. Analyse und Codegen sind in Nikaia geschrieben. | nur Nikaia-Programme |
| **B** Laufzeit in Nikaia | `rt`, Fehler, Interner, Scans als Nikaia-Code, über Pre-Lowering (ADR-290 D16) für Rust nutzbar | Rust und Nikaia |
| **C** Modell und Analyse in Nikaia | Validator, FIRST-Mengen, Frame-Prüfung in Nikaia. Das Proc-Macro bleibt eine dünne Rust-Hülle, die den abgesenkten Code aufruft. | Rust und Nikaia |
| **D** vollständig, mit eigener Engine | Nikaia ersetzt winnow und winnow-grammar ganz | nur Nikaia, als Paket für Rust erst nach #978 |
| **E** Mischform | z. B. A und B | |

**Diese Wahl müssen wir zuerst treffen** (siehe Abschnitt 3). Die Fragen
unten sind so formuliert, dass sie für jede Variante beantwortet werden
können.

---

## 1. Fragen aus Sicht der Sprache Nikaia

Welche Eigenschaften der Sprache müssen wir untersuchen, um die Leitfrage
überhaupt beantworten zu können?

### 1.1 Metaprogrammierung und Codeerzeugung

*Fakten:* Nikaia hat keine Makros und kein Quoting. Es hat `comptime`, das
in einem Interpreter läuft (ADR-002 D2), Reflexion über
`T::fields`/`T::variants`/`T::kind` (ADR-330), `asset(…)` und
`dsl … eod`-Blöcke (Part II 10.5). Eine Grammatik, die zur Build-Zeit läuft,
wird aus dem generierten Parser kompiliert (ADR-177).

1. Kann Nikaia-Code **zur Build-Zeit Code erzeugen**, der danach kompiliert
   wird? Oder ist das Sprachkonstrukt `grammar` die einzige Stelle, an der
   der Compiler selbst generiert?
2. Wenn der Generator in Nikaia steht (Variante A): Ist das ein Teil des
   Compilers (Lowering), ein `comptime`-Programm oder ein separates Werkzeug
   (`tools/*.nika`)?
3. Gibt es ein Gegenstück zu `syn::Error` mit Span, also Fehler, die auf die
   Stelle in der Grammatik zeigen? Und zum `DEBUG_GRAMMAR`-Ausdruck des
   erzeugten Codes?
4. Verstößt eine Codeerzeugung in Nikaia-Bibliothekscode gegen ADR-298 D6
   (kein Makrosystem)? Oder fällt sie unter `comptime`/`dsl`?

### 1.2 Typsystem: Generik, Traits, Sichten statt Lebensdauern

*Fakten:* Generik schreibt man aus, sie wird nicht inferiert (Part I 4.6).
Es gibt Traits (4.7) und `impl` ohne Trait nur für eigene Typen (NK1209).
Lebensdauer-Annotationen gibt es **nie** (Part I 6.5), Sichten heißen
`ref X` und werden angebunden (6.6, ADR-283). Ein Struct mit
Lebensdauer-Parametern gibt es nicht. Bei gespeicherten `&str` ist ein
E0621-Problem offen (`nk:docs/stored-views.md`). Closures schreibt man als
`fn { … }`; über Zyklen werden sie zu `&dyn Fn`.

5. winnow-grammar arbeitet mit `ParseInput<'a, S>`, Ausgaben, die vom Input
   borgen (`&'a str`), zwei Lebensdauern im Kontext (ADR 19) und
   `__WgS`/`__WgEr`-Parametern. **Lassen sich diese Signaturen in Nikaia
   ausdrücken**, und was kommt beim Lowering heraus? Betrifft das
   E0621-Problem genau diesen Fall?
6. Kann Nikaia das „State-Typ festnageln“ (ADR 20) und den austauschbaren
   Interner (ADR 22, `Interner`/`InternerOf`/`StateOf`) ausdrücken, ohne
   Assoziativtypen-Tricks?
7. Was kosten `fn { … }`-Closures, die als `&dyn Fn` abgesenkt werden, im
   heißen Pfad von Kombinatoren? Gibt es eine monomorphisierte Form?
8. Wie bildet Nikaia `Fn(T,T)->T`-Merges und die Kontext-Fabrik
   `impl Fn() -> ParseContext<S> + Sync` ab (Thread-Grenzen, NK2502/2503)?

### 1.3 Zustand in Grammatiken

*Fakten:* ADR-339 D3: „A grammar declares no state and no `extern rule`“.
ADR-383 verlegt Knoten-IDs in eine Nummerierungs-Phase. winnow-grammar hat
dagegen `ParseContext<S>`, `state MyState;`, `extern rule` und Interning.

9. Ist ADR-339 D3 eine Grenze der **Nikaia-Grammatiken** oder des
   **Generators**? Müsste ein Generator in Nikaia Zustand und `extern rule`
   anbieten, damit Rust-Nutzer nichts verlieren?
10. Wie wird Interning (`ident`, `intern(…)`, lasso `ThreadedRodeo`) in
    Nikaia ausgedrückt, wenn Grammatiken keinen Zustand haben?

### 1.4 Sicherheit, `unsafe`, Verifikation

*Fakten:* Die Laufzeit hat 26 `unsafe`-Blöcke (`from_utf8_unchecked`,
`get_unchecked`, `*_slice_unchecked`); der generierte Code hat keine. In
Nikaia liegt `unsafe` in `crates/unsafe/*` (ADR-218), `assume` nur in
`unsafe`. Bewiesen werden lineare Arithmetik, Indizes (54 % auf dem eigenen
Code), Schleifeninvarianten und Vorbedingungen. **Nicht** bewiesen werden
Terminierung, Strings, Quantoren über Listen und Rekursion
(`nk:docs/formal-verification.md` §2). „Parser gegen formale Grammatik“ ist
dort mit B bewertet, ein Beweis des Parsers mit D. Bewiesene Prüfungen
dürfen entfallen (ADR-306).

11. Welche der 26 `unsafe`-Stellen könnte Nikaias Beweiser durch **bewiesene
    Indizes** ersetzen (ADR-306)? Ist das sicher **und** gleich schnell?
12. Welche Eigenschaften einer Grammatik oder eines Parsers will man
    beweisen: kein Panic, Indizes im Rahmen, Fortschritt pro Wiederholung
    (gegen Endlosschleifen), Frame-Grenzen? Welche davon liegen im heutigen
    Beweisbereich, welche (Terminierung) nicht?
13. Kann der Beweiser Aussagen über den **erzeugten** Parser treffen, oder nur
    über den Generator selbst?
14. Überlaufprüfungen sind in Nikaia Pflicht. Was kosten sie in Scans und
    Zählern (z. B. `count(p)`, TODO §4)?

### 1.5 Leistung

*Fakten:* Scannen hat in Nikaia eine spezifizierte Komplexität, nie Byte für
Byte (ADR-296 D12). `Bytes::find` ist memchr (ADR-320 D6), dazu kommt
`simd::Fixed[u8,32]`. 1BRC, Instruktionen pro Zeile:
`1brc.nika` (Grammatik + `par_fold`) 211, `fast.nika` safe 140, optimiert 95,
getuntes Rust 147–271. Bekannte Engine-Kosten: `advance` im Profil,
Literal-Alternativen ohne Erstbyte-Switch (staging-candidates §2.1).

15. Kann Nikaia-Code die Tricks der Laufzeit **gleich gut** ausdrücken: SWAR
    pro Wort (ADR 23), ein `static` AVX2-Searcher pro Scan-Stelle (ADR 24
    §12b), Matching per Index bei begrenzten Formaten (§8)?
16. Variante A: Könnte ein Generator im Compiler **mehr Wissen nutzen** als
    ein Proc-Macro (Typen, bewiesene Grenzen, `comptime`-Werte, #906
    „Spezialisierung pro Build-Zeit-Wert“) und dadurch schnelleren Code
    erzeugen?
17. Wie messen wir den Vergleich nach den Messregeln (Instruktionen per
    callgrind, gleiche Ausgabe, Release-Builds auf beiden Seiten)?

### 1.6 Interop und Verteilung

*Fakten:* Rust kann Nikaia-Code nur über committetes Pre-Lowering nutzen
(ADR-290 D16/D17) oder als C-Bibliothek. Ein Artefakt `rust-library` fehlt
noch (#978). ADR-290 D17/D12 schließt „anything the compiler needs in order
to compile itself“ von diesem Weg aus.

18. Kann eine **Rust-Bibliothek** (also winnow-grammar für Rust-Nutzer
    außerhalb von Nikaia) heute überhaupt in Nikaia geschrieben und
    ausgeliefert werden? Was fehlt bis #978?
19. Schließt ADR-290 D17/D12 genau diesen Fall aus? Der Compiler braucht den
    Parser-Generator ja, um sich selbst zu übersetzen (Bootstrapping).
20. Wie sieht das Bootstrapping aus: Mit welchem Parser wird der Generator
    übersetzt, wenn er selbst in Nikaia mit `grammar` geschrieben ist?
    Genügen eingecheckte, abgesenkte `.rs`-Dateien (ADR-294 D1)?
21. Lösen wir damit die crates.io-Sperre (ADR-336), oder verschieben wir sie
    nur?

### 1.7 Reife und Lücken der Sprache

*Fakten:* Version 0.0.1317, Status „specification + bootstrap“,
Self-Hosting bei 78–82 %. Offene Grammatik-Defekte: #999, #1001–#1004, #928
(Streaming wird noch nicht abgesenkt), #557/#903 (der Compiler-Parser selbst
wird zur Nikaia-Grammatik). Für fehlende Formen gilt die Regel: Arbeit
stoppen und eskalieren (`CLAUDE.md`).

22. Welche **Sprach- und `std`-Lücken** würde eine Portierung von etwa
    18.500 Zeilen aufdecken (Kategorien 1/2/3 nach `CLAUDE.md`)? Stichprobe:
    Hash-Maps, Interner, `Arc`, Threads, `std::io::Read`, Bit-Operationen.
23. Wie verhält sich das Vorhaben zur laufenden Arbeit (ADR-339-Grammatik,
    #557) und zur Priorisierung in `docs/priorities.md`?
24. Ist eine Portierung ein **guter Prüfstein** für die Sprache, also im
    Sinne des Prinzips „Helper tools are written in Nikaia … shows where it
    has to be sharpened“?

---

## 2. Fragen aus Sicht von winnow-grammar

Welche Vorteile hätte es für die Bibliothek selbst, in Nikaia geschrieben zu
sein, und was würde sie verlieren?

### 2.1 Zielgruppe und Identität

*Fakten:* winnow-grammar beschreibt sich als „parser generator for Rust“
mit `grammar!` im Rust-Code, Rust-Aktionsblöcken `-> { … }` und
Veröffentlichung auf crates.io. Faktisch ist Nikaia der Haupttreiber.

25. Für wen ist winnow-grammar da: für Rust-Nutzer allgemein, für Nikaia,
    oder für beide? Bleibt es eine **Rust-Bibliothek**?
26. Darf winnow-grammar eine Abhängigkeit (oder einen Build-Schritt) zu
    Nikaia bekommen? Was hieße das für Rust-Nutzer, die Nikaia nicht kennen
    (ADR-290 D17: „available to Rust programs that have never heard of
    Nikaia“)?

### 2.2 Aktionen und eingebettetes Rust

*Fakten:* Aktionen sind opake `TokenStream`s. Fold-`init`/`step`/`merge`,
Guards und Werte sind `syn::Expr`/`syn::Type`. Codegen ersetzt
Typparameter mit `syn::visit_mut` und entscheidet Inlining per
Token-Scan (`codegen/fixed.rs:77`).

27. In welcher Sprache stünden Aktionen? Rust-Aktionen kann ein
    Nikaia-Generator nur als opaken Text behandeln. Nikaia-Aktionen
    schließen Rust-Nutzer aus.
28. Würden wir die Analysefähigkeit verlieren, die heute auf `syn` beruht
    (Typparameter-Ersetzung, Inlining-Entscheidung)? Oder würden wir sie
    gewinnen, weil ein Nikaia-Generator die Typen der Aktionen kennt?

### 2.3 Korrektheit der Bibliothek

*Fakten:* Prüfungen heute: Nullable-Fixpunkt, indirekte Linksrekursion wird
abgelehnt, direkte umgeschrieben, Präfix- und Shadowing-Mehrdeutigkeit über
FIRST-Mengen, Frame-Behauptungen. **Terminierung und Mehrdeutigkeit sind
nicht formal geprüft.** Offen: positionsabhängige Erreichbarkeit (TODO §1).

29. Welche bestehenden Prüfungen (`analysis.rs`, `validator.rs`,
    `frame.rs`) würden **durch Nikaias Beweiser** sicherer oder einfacher?
30. Welche heute fehlenden Garantien könnte man **neu** gewinnen (z. B.
    „jede Wiederholung verbraucht Eingabe“, Frame-Grenzen bewiesen statt
    geprüft, kein Panic im Laufzeitcode)?
31. Würde die `unsafe`-freie Erzeugung (ADR 24 §12a) zu einer **bewiesenen**
    Laufzeit, oder bleibt `crates/unsafe` die Grenze?

### 2.4 Leistung der Bibliothek

32. Hilft Nikaia bei den offenen Leistungsfragen (TODO §2 Merges über
    Stücke / ADR 21, TODO §4 `count(p)`, Erstbyte-Switch bei Literalen)?
    Oder sind das Entwurfsfragen, die von der Sprache unabhängig sind?
33. **Übersetzungszeit:** Proc-Macros mit `syn full` kosten Kompilierzeit.
    Bisher ist das nicht gemessen (ADR 18 Z. 159). Würde ein Generator im
    Nikaia-Compiler das verbessern? Dafür bräuchten wir zuerst eine Messung
    des heutigen Stands.

### 2.5 Entwicklungsprozess und Wartung

34. Verschwindet die Reibung „location, not difficulty“, wenn Generator und
    Compiler in einem Repository stehen? Ginge das auch **ohne
    Sprachwechsel** (vendoren, Workspace-Mitglied)?
35. Was wird aus den 397 Rust-Tests und 13 trybuild-Fällen? Nach der
    Nikaia-Regel #821 sind neue Tests nur in Nikaia erlaubt. Wie testet man
    dann die Rust-API?
36. Was wird aus der **öffentlichen API**, die erhalten bleiben muss
    (`parse_<rule>`, `_pieces`, `_pieces_with`, `_reader`, `_reader_each`,
    `ParseContext`, `Symbol`, `Diagnose`, `testing`, Features `rayon`/`ahash`/
    `trace`)?
37. Wer pflegt zwei Sprachen in einem Projekt, und was kostet das
    (Toolchain, CI, Einstieg für neue Mitwirkende)?

### 2.6 Abhängigkeiten

38. Welche der Abhängigkeiten (`winnow`, `memchr`, `lasso`, `ahash`,
    `rayon`, `syn`, `quote`, `proc-macro2`, `paste`) entfallen, welche
    braucht man weiter als Rust-Adapter (ADR-294 D5), und welche müsste
    Nikaias `std` liefern?
39. Bleibt `winnow` die Engine (Variante A–C), oder wäre eine eigene Engine
    in Nikaia (Variante D) ein Vorteil, etwa weil sie besser zum Beweiser
    oder zu den Nikaia-Byte-Typen passt?

---

## 3. Vorschlag für das weitere Vorgehen (zur Besprechung)

1. **Variante festlegen (0.3).** Wichtigste Entscheidung: Geht es um
   Nikaias eigenen Generator (A), um eine Rust-Bibliothek, deren Teile in
   Nikaia stehen (B/C), oder um einen vollständigen Ersatz (D)? Davon hängt
   ab, welche der Fragen 18–21 und 25–28 überhaupt zählen.
2. **Harte Ausschlusskriterien zuerst prüfen**, weil sie Varianten früh
   streichen können: Fragen 1, 5, 18, 19, 20, 27.
3. **Zwei kleine Proben** statt Theorie:
   * ein Stück Laufzeit (z. B. `rt::scan_to_*` oder `take_in_bounds`) in
     Nikaia, mit Instruktionszählung gegen das Rust-Original und der Frage,
     ob der Beweiser die `unsafe`-Indizes ersetzt (Fragen 11, 15);
   * ein Stück Analyse (z. B. Nullable-Fixpunkt oder FIRST-Mengen) in
     Nikaia, um Lücken der Sprache zu finden (Frage 22).
4. **Kompilierzeit von `grammar!` heute messen** (Frage 33). Das ist
   unabhängig vom Ergebnis nützlich.
5. Danach Vorteile und Kosten je Variante gewichten und in einem zweiten
   Dokument beantworten.

Offene Fragen an dich:

* Welche Variante(n) aus 0.3 meinst du, oder sollen wir alle bewerten?
* Ist „winnow-grammar bleibt für Rust-Nutzer ohne Nikaia benutzbar“ eine
  feste Bedingung?
* Sollen die Proben aus 3.3 schon im nächsten Schritt gebaut werden, oder
  zuerst nur die Ausschlusskriterien am Papier geprüft werden?
