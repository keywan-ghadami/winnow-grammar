# winnow-grammar in Nikaia: Was eine Migration heute verhindert (Schritt 2)

**Ziel (geklärt am 2026-10-31):** Der Parser-Generator wird in Nikaia
geschrieben und kommt ohne Rust-Makros aus. Er erzeugt Parser, und Nikaia
benutzt diesen in Nikaia geschriebenen Generator auch selbst, für die eigenen
Grammatiken und den eigenen Compiler-Parser.

**Was dieses Dokument enthält:** einen Katalog der Punkte, die das heute
verhindern. Zu jedem Punkt steht, was eine Änderung des heutigen Zustands
**allgemein** für die Sprache brächte, über dieses Ziel hinaus, und was sie
kosten würde. Das soll eine Entscheidung pro Punkt möglich machen. Die
Fakten und Quellen stehen in Schritt 1
(`docs/analysis-winnow-grammar-in-nikaia.md`). Pfade mit `nk:` liegen im
Nikaia-Repository.

**Wie die Punkte eingeordnet sind:**

* **Vorteil**: Die Änderung ist ohnehin gewollt, ein Nachteil ist nicht
  erkennbar. Hier ist nur über die Priorität zu entscheiden.
* **Defekt/Lücke**: Etwas ist spezifiziert oder klar gemeint und fehlt oder
  geht nicht. Es wird repariert, eine Grundsatzfrage gibt es nicht.
* **Abwägung**: Eine bestehende Regel oder Entscheidung müsste fallen oder
  sich ändern. Es gibt echte Vor- und Nachteile, die Entscheidung liegt beim
  Owner.
* **Kein Hindernis**: Der Punkt ist lösbar oder nur scheinbar ein Problem.
  Er steht hier, damit er nicht vergessen wird.

---

## Übersicht

| # | Punkt | Einordnung | Meine Tendenz |
| :--- | :--- | :--- | :--- |
| H1 | Wo der Generator läuft (ADR-296 D23: Lowering schreibt `grammar!`) | Abwägung, klein | ändern: Generator im Compiler |
| H2 | Eine Grammatiksprache oder zwei (Nikaia-`grammar` vs. winnow-grammar-DSL) | Abwägung | eine: Nikaia-`grammar` |
| H3 | Was der Generator erzeugt (Rust + winnow, Nikaia-Code, eigene Engine) | Abwägung | offen, Probe nötig |
| H4 | Aktionen in Rust | Abwägung, die zentrale Frage | Aktionen in Nikaia; für Rust ein typisiertes Ergebnis |
| H5 | Grammatiken ohne Zustand und ohne `extern rule` (ADR-339 D3) | Abwägung | Regel halten, Teilöffnung prüfen |
| H6 | Fremde Rust-Traits implementieren (#928 Option b) | Abwägung | vorerst nein (Fassade), später neu fragen |
| H7 | Rust-Bibliothek ausliefern (#978) | Vorteil | priorisieren |
| H8 | ADR-290 D12/D17: Was der Compiler braucht, darf nicht über diesen Weg | Vorteil (Grund überholt durch ADR-294) | Regel an ADR-294 angleichen |
| H9 | Henne-Ei / Bootstrapping | kein Hindernis | wie beim Self-Hosting |
| H10 | Gespeicherte Sichten / Lebensdauern (E0621, #1000, #1037, #1038) | Defekt/Lücke | reparieren, Regel „keine Lebensdauern“ bleibt |
| H11 | `unsafe` und Leistungsprimitive der Laufzeit | Lücke + Abwägung (Beweiser für Text) | erst Probe, dann entscheiden |
| H12 | Lücken in `std` und Sprache (Maps, `mem::take`, nebenläufige Tabellen …) | meist Defekt/Lücke, eine Abwägung (NK1307) | entschiedene Tickets priorisieren |
| H13 | Tests: Nikaia-only (#821) und die Rust-API | kein Hindernis, Prozessfrage | Tests in Nikaia, Rust-Seite über #978-Test |

---

## H1 Wo der Generator läuft

**Heute:** Nikaias `grammar` wird syntaktisch zu `grammar! { … }`
abgesenkt (ADR-296 D17, D23). Die Parser-Entscheidungen trifft das Rust-Makro
in winnow-grammar.

**Was die Migration braucht:** Das Lowering von `grammar` erzeugt den
Parser selbst. Der Generator ist Nikaia-Code (z. B. `tools/grammar_*.nika`),
den der Compiler aufruft. Dafür ist **keine Sprachänderung** nötig, nur
ADR-296 D23 ändert sich. Nikaia bleibt ohne Makros (10.3, ADR-298): Der
Generator ist Teil des Compilers, kein Bibliothekscode, der zur Build-Zeit
Code schreibt.

**Die Alternative wäre eine Sprachänderung:** Bibliotheken dürften zur
Build-Zeit Code erzeugen, etwa per `comptime`, das Quelltext zurückgibt.
Dann könnte der Generator ein gewöhnliches Paket sein.

| Vorteile einer Änderung von D23 | Nachteile |
| :--- | :--- |
| Die Reibung „location, not difficulty“ (staging-candidates §2.1) entfällt: Grammatik-Defekte werden dort repariert, wo sie entstehen. | Mehr Code im Compiler, auch im Self-Hosting-Anteil. |
| Der Generator kennt Typen, bewiesene Grenzen und `comptime`-Werte (#906): Er kann besseren Code und bessere Fehler (NK-Codes, `.nika`-Spans) erzeugen als ein Makro, das nur Tokens sieht. | D17 („never guesses a meaning“) muss neu gefasst werden, denn der Generator *versteht* dann die Grammatik. |
| Die Git-Abhängigkeit (ADR-336, crates.io) fällt weg. | |
| Self-Hosting rückt vor: Die größte Rust-Abhängigkeit des Compilers wird Nikaia. | |

| Codeerzeugung durch Bibliotheken (Alternative) | |
| :--- | :--- |
| Vorteil, allgemein: Andere Generatoren (Protobuf, SQL-Schemas, Serialisierung) wären Pakete und nicht Compiler-Teile. | Nachteil: Das ist ein Makrosystem durch die Hintertür. Es widerspricht 10.3/10.4 (keine Hygienefrage, „nothing injected“), erschwert Werkzeuge (IDE, `fmt`, Beweiser sehen erzeugten Code erst spät) und macht Build-Zeit-Code mächtiger (NK1152-Grenze). |

**Tendenz:** D23 ändern, den Generator in den Compiler legen, keine
Codeerzeugung durch Bibliotheken.

**Folge für Rust-Nutzer ohne Nikaia:** Sie haben kein `grammar!` mehr. Sie
bekommen den Parser über ein Werkzeug (`nikaia` als CLI oder in `build.rs`)
oder als fertiges Paket über #978 (H7). Ob das für winnow-grammar als
Rust-Produkt reicht, ist eine Produktentscheidung (siehe H4).

## H2 Eine Grammatiksprache oder zwei

**Heute** gibt es zwei Sprachen. Die Nikaia-`grammar` hat Commit-Punkte
`=>`, Klein-/Großschreibung für syntaktische und lexikalische Regeln, keinen
Zustand und kein `extern rule` (Part II 10.1–10.8, ADR-339). Die DSL von
winnow-grammar (`SYNTAX.md`, 1.597 Zeilen) hat `state`, `extern rule`,
Frames, `#[stream]`, Rust-Aktionen und anderes. Das Lowering übersetzt die
erste in die zweite.

**Entscheidung:** Was liest der Nikaia-Generator ein?

| Nur Nikaia-`grammar` | Beide |
| :--- | :--- |
| + eine Spezifikation, ein Parser, eine Fehlerliste | + bestehende Rust-Grammatiken laufen weiter |
| + passt zur Regel „eine Regel an einem Ort“ (`writing-guide.md`) | − zwei Semantiken pflegen, z. B. Zustand ja/nein (H5) |
| − Rust-Nutzer müssen ihre Grammatiken umschreiben (Konverter möglich, vgl. `rs2nika`) | − was winnow-grammar hat und Nikaia verbietet, müsste doch erlaubt werden |

**Tendenz:** nur Nikaia-`grammar`. Was der winnow-grammar-DSL heute voraus
ist, geht als Einzelpunkt in H4/H5 ein, nicht als zweite Sprache.

## H3 Was der Generator erzeugt

Drei Möglichkeiten, die sich nicht gegenseitig ausschließen:

1. **Rust-Code, der winnow aufruft** (wie heute, nur ohne Makro). winnow
   bleibt ein „Adapter zu einem Crate“ (ADR-294 D2/D5). Das ist der
   kleinste Schritt. Der erzeugte Code ist aber Rust und damit für Nikaias
   Beweiser unsichtbar.
2. **Nikaia-Code** (Parser-Funktionen in `.nika`), der dann normal
   abgesenkt wird. Der erzeugte Parser wird geprüft, bewiesen
   (Indexgrenzen, Überlauf) und läuft in `comptime` (ADR-177). Dafür muss
   Nikaia die Laufzeittricks selbst ausdrücken können (H11).
3. **Eigene Engine in Nikaia** statt winnow. Das ist die Konsequenz von 2,
   wenn die Laufzeit (`rt`, Scans, Fehler-Ranking) ebenfalls Nikaia wird.

| Vorteile von 2/3 (allgemein) | Nachteile |
| :--- | :--- |
| Jeder Nikaia-Parser (json, yaml, http1, Compiler) bekommt Beweise und `unsafe`-freien Code ohne Vertrauen in Fremd-Rust. | Höchstes Leistungsrisiko: 1BRC zeigt `1brc.nika` mit Grammatik bei 211 Instr./Zeile, hand-optimiertes `fast.nika` bei 140. Woher die Differenz kommt (Engine oder Sprache), ist offen. |
| Ein starker Prüfstein: Jede Lücke im Leistungscode wird ein Fund für den Compiler (Prinzip aus `CLAUDE.md`). | Größter Umfang (rund 7.000 Zeilen Laufzeit). |
| winnow als Abhängigkeit entfällt. | Die Erfahrung von winnow (Fehlerobjekte, Kombinatoren) muss neu gebaut werden. |

**Tendenz:** offen. Ich schlage eine Probe vor: ein Laufzeitstück
(`rt::scan_to_*`, `take_in_bounds`) in Nikaia, gezählt gegen das Original.
Danach entscheiden. Schritt 1 ist möglich, ohne 2/3 zu verbauen.

## H4 Aktionen in Rust

**Heute:** Aktionen `-> { … }` sind opake Rust-`TokenStream`s. Dazu kommen
Fold-`init`/`step`/`merge`, Guards und Werte als `syn::Expr`. Codegen
ersetzt darin Typparameter (`syn::visit_mut`) und entscheidet Inlining über
einen Token-Scan (`codegen/fixed.rs:77`). In Nikaia-Grammatiken sind
Aktionen Nikaia-Code, und unter `comptime` darf kein Rust laufen (Part II
10.2, NK1152).

**Optionen:**

| Option | Vorteile | Nachteile |
| :--- | :--- | :--- |
| **a)** Rust-Aktionen als opaker Text durchreichen (nur bei Ziel 1 in H3) | Rust-Nutzer behalten ihre Grammatiken fast unverändert. | Der Generator versteht die Aktionen nicht mehr (heute versteht `syn` sie teilweise). Eine Grammatik mit Rust-Aktionen ist nicht `comptime`-fähig. Es entsteht eine zweite Grammatiksprache (H2). Das widerspricht dem Gedanken, dass der Generator *mehr* weiß. |
| **b)** Nur Nikaia-Aktionen; Rust-Nutzer bekommen über #978 ein Paket | Eine Sprache, alles geprüft und `comptime`-fähig. Rust-Nutzer sehen eine normale Bibliothek. | Rust-Nutzer müssen ihre Aktionen in Nikaia schreiben, also Nikaia benutzen. Das ist für ein Rust-Produkt eine hohe Hürde. |
| **c)** Nikaia-Aktionen, die Rust-Funktionen aufrufen (`type = "rust"`-Abhängigkeit mit `describe`, Part III 15.2) | Vorhandener Mechanismus. Rust-Logik bleibt in Rust. | Nicht unter `comptime` (NK1152). Typen über die Grenze nur nach Abbildungsregeln (NK2502/2503). |
| **d)** Grammatik ohne Aktionen, für Rust ein **typisiertes Ergebnis** (Baum oder Ereignisse mit Spans), das der Nutzer in Rust abbildet | Der Rust-Nutzer schreibt nur die Grammatik (Nikaia-Syntax) und reines Rust. Kein Nikaia-Code beim Nutzer. Erzeugt ohnehin wertvolle Dinge (Ereignis-Parser, Linter, LSP; vgl. #928 „pull/event parser with spans“). | Ein Baum kostet Allokationen. Zero-Copy-Sichten brauchen H10. Etwas langsamer als direkte Aktionen. |

**Allgemeine Sprachfrage dahinter:** Soll Nikaia Fremdsprachen-Code in
Aktionen zulassen (a)? Das wäre ein Präzedenzfall für „Rust-Blöcke in
Nikaia“. Er würde die Sicherheitsaussagen (kein fremder Code zur Build-Zeit,
Beweise) aufweichen.

**Tendenz:** b für Nikaia-Nutzer, d (bei Bedarf c) für Rust-Nutzer, a
nicht. **Das ist die Entscheidung mit der größten Wirkung auf
winnow-grammar als Rust-Produkt**: Mit b/d gibt es kein Rust-Crate
„winnow-grammar“ mehr im heutigen Sinn, sondern Nikaia-Grammatiken mit
Rust-Ausgabe.

## H5 Grammatiken ohne Zustand und ohne `extern rule` (ADR-339 D3)

**Heute:** Nikaia-Grammatiken haben keinen Zustand und keine externen Regeln.
Knoten-IDs werden in einem eigenen Durchlauf vergeben (ADR-383). Interning
gibt es trotzdem (`intern(p)`, `ident`, Part II 10.8, Z. 739; die Tabelle
hält der Parser, 10.6). winnow-grammar hat `ParseContext<S>`, `state
MyState;`, austauschbare Interner (ADR 20, 22) und `extern rule`.

| Vorteile einer Öffnung (allgemein) | Nachteile |
| :--- | :--- |
| Volle Ausdruckskraft für Grammatiken, die Kontext brauchen (Einrückung bei YAML, Symboltabellen in C-artigen Sprachen, `typedef`-Problem). | Zustand und Backtracking passen schlecht zusammen: Bei jedem verworfenen Zweig muss der Zustand zurückgerollt werden, oder er ist falsch. Heute schützt die Regel davor. |
| Ein Durchlauf weniger (IDs, Tabellen beim Parsen). winnow-grammar misst das Interning mit 2,5–3,6 % eines Compiler-Laufs. | Das Zusammenführen bei `par_fold` wird schwer: Genau dieses Problem ist in ADR 21/TODO §2 ungelöst. Ohne Zustand gibt es es nicht. |
| `extern rule`: handgeschriebene Spezialparser (Zahlen, Base64, SIMD-Scans) einhängen. | Dual-Mode (10.2): Zustand muss in `comptime` und zur Laufzeit gleich wirken. Mehr Prüfregeln. |
| | `extern rule` in Rust widerspricht NK1152 (kein Rust zur Build-Zeit). In Nikaia ist es nur ein Funktionsaufruf. |
| | Der Beweiser hat es mit reinen Funktionen leichter. |

**Mögliche Teilöffnungen:**

* **Nur lesbarer Kontext:** Parameter, die eine Regel weiterreicht.
  Rule-Parameter gibt es schon (yaml).
* **`extern rule` nur für Nikaia-Funktionen** mit fester Signatur.
* **Interning als einziger eingebauter Zustand.** Es ist schon da, nur nicht
  austauschbar.

**Tendenz:** D3 halten. Einzeln prüfen, ob `extern rule` auf
Nikaia-Funktionen beschränkt geöffnet wird. Die Frage ist erst dringend,
wenn eine Grammatik ohne Zustand ein reales Ziel nicht erreicht. Der
Compiler-Parser kommt nach ADR-339/383 ohne Zustand aus.

## H6 Fremde Rust-Traits implementieren (#928, Option b)

**Heute:** Nikaia kann keine fremden Rust-Traits implementieren
(`serde::Deserialize`, `std::io::Read`, `winnow::Parser`). Für yaml ist
deshalb entschieden: Ein Kern in Nikaia und eine handgeschriebene
Rust-Fassade (#928, Option a).

**Bezug zum Ziel:** Eine idiomatische Rust-API des Generators oder der
erzeugten Parser braucht Traits wie `Read` oder `Error`. Bei Ziel 1 in H3
muss der erzeugte Code mit winnow-Traits arbeiten. Das schreibt aber der
Generator als Text, nicht die Sprache.

| Vorteile (allgemein) | Nachteile |
| :--- | :--- |
| Rust-Fassaden entfallen überall (yaml, json, toml, csv). Ganze Crates sind Nikaia, ganz im Sinn von „Nikaia zeigt, was es kann“. | Große Sprachänderung. Rust-Semantik (Lebensdauern in Signaturen, `'de`) dringt in die Sprache ein, und das reibt sich mit „keine Lebensdauern, nie“ (Part I 6.5). |
| | Bindet die Sprache an das Rust-Backend (C-Bibliothek, wasm, Python- und JS-Bindungen haben andere Traits). |

**Tendenz:** vorerst nein. Eine dünne Fassade wie bei yaml reicht auch hier.
Neu fragen, wenn yaml und json zeigen, wie viel die Fassaden kosten.

## H7 Rust-Bibliothek ausliefern (#978)

**Heute:** Ein Paket wird ein Programm oder eine C-Bibliothek, kein
Rust-Crate. Die Entscheidung ist gefallen (Owner, 2026-10-29), das Ticket
ist `ready`. Es blockiert auch #928 (yaml, Schritte 8 und 9).

**Vorteile:** Jedes Nikaia-Paket wird für Rust nutzbar (yaml, json und der
Generator selbst). Features ohne `#[cfg]` im Code.

**Nachteile:** keine erkennbaren, außer dem Bauaufwand.

**Einordnung: reiner Vorteil, priorisieren.**

## H8 ADR-290 D12/D17: „anything the compiler needs … stays out“

**Heute:** D17 schließt Teile, die der Compiler selbst zum Übersetzen
braucht, vom Weg „ein Stück Nikaia, das Nikaia- und Rust-Programme
gleichermaßen aufrufen“ aus. Der Generator wäre genau so ein Teil.

**Woher die Regel kommt** (`nk:docs/history/adr/adr-195.md` D2,
`adr-196.md` D2, später ADR-290 D12/D17, 2026-10-03): Die Begründung ist
nur der **Bootstrap-Zyklus**. `describe` durfte als erstes Werkzeug in
Nikaia geschrieben werden, *weil* der Compiler es nicht braucht: „Nothing
bootstraps through it … So there is no cycle to break.“ Ob der Compiler
selbst je Nikaia wird, ließ ADR-196 §4 ausdrücklich offen. Ebenso offen
blieb, ob ein Nikaia-Modul für Rust-Nutzer veröffentlicht wird („whether
this project wants to carry that promise is not asked here“).

**Seitdem ist beides entschieden:**

* **ADR-294:** Der Compiler zieht nach Nikaia um, über genau diesen Weg
  (committetes Pre-Lowering, D1). Er übersetzt sich heute schon mit
  abgesenktem Nikaia-Code (`tools/*.nika`, Self-Hosting 78–82 %). Der
  Zyklus ist dort gelöst.
* **#928/#978:** Nikaia-Pakete werden als Rust-Crates veröffentlicht.

Der Grund der Regel ist also überholt. Außerdem trennt sie zwei Dinge
nicht: Ob ein Zyklus entsteht, hängt davon ab, ob *der Compiler* das Stück
braucht, nicht davon, ob *zusätzlich* Rust-Nutzer es aufrufen. Ein
weiterer Aufrufer erzeugt keinen Zyklus.

**Einordnung: Vorteil.** Die Regel wird an ADR-294 angeglichen; D17 nennt
dann nur noch die Bedingung „Text oder Bytes rein, Daten raus, Aufrufer auf
beiden Seiten“. Die Kosten des Teilens bestehen unabhängig von der Regel
und werden anders behandelt:

* **API-Versprechen:** Ein Teil, den Rust-Nutzer aufrufen, kann der Compiler
  nicht mehr frei umbauen. Das regeln die Versionen aus #978, wie bei
  jedem veröffentlichten Paket.
* **Ein Fehler trifft beide Seiten.** Eine Änderung für Rust-Nutzer ist
  zugleich eine Compiler-Änderung. Dagegen hilft der Fixpunkt-Test aus
  H9, nicht die Regel.

## H9 Henne-Ei / Bootstrapping

**Kein Hindernis.** Es ist dasselbe Muster wie beim Self-Hosting: Der
Generator ist `.nika`. Sein abgesenktes `.rs` wird committet (ADR-294 D1,
ADR-002 D4), und der Compiler baut sich mit der eingecheckten Fassung.

Zwei Punkte gehören trotzdem in die ADR:

* **Ein Generator-Fehler trifft den Compiler-Parser.** Eine fehlerhafte
  neue Fassung darf das committete `.rs` nicht überschreiben, bevor sie sich
  selbst übersetzt hat (Fixpunkt-Test: Generator v2 erzeugt aus sich
  selbst dasselbe wie v1 aus v2).
* **Der Generator wird selbst mit `grammar` geschrieben**, um die
  Grammatik-Syntax zu lesen. Das ist der doppelte Zyklus. Er ist mit
  derselben Regel gelöst, braucht aber einen Test.

## H10 Gespeicherte Sichten / Lebensdauern

**Heute:** Nikaia hat keine Lebensdauer-Annotationen, „ever“ (Part I 6.5).
Sichten werden abgeleitet (ADR-296 D21, ADR-283). Offen sind:

* E0621 bei einer gespeicherten `&str` (`nk:docs/stored-views.md`, mit
  einer offenen Frage an den Owner);
* #1000 (Struct aus Sichten eines `ref`-Parameters, blockiert yaml);
* #1038.

#1037 ist gebaut.

**Bezug:** Ein Generator mit Zero-Copy-Ergebnissen (Ausgaben, die vom Input
borgen), Tokens und AST-Knoten mit Textsichten sowie Interner mit
`&'a str` braucht genau das.

**Einordnung: Defekt/Lücke.** Die Regel „keine Annotationen“ zu kippen,
brächte hier nichts, was die Inferenz nicht leisten soll, und würde eine
zentrale Zusage der Sprache brechen. Die Inferenz ist zu vervollständigen.
Die offene Frage in `stored-views.md` ist eine **Owner-Entscheidung**, und
ihr Ergebnis bestimmt, wie weit Zero-Copy im Generator reicht.

## H11 `unsafe` und Leistungsprimitive der Laufzeit

**Heute:** Die Laufzeit hat 26 `unsafe`-Blöcke (`from_utf8_unchecked`,
`get_unchecked`, `*_slice_unchecked`). Sie nutzt SWAR pro Wort (ADR 23), einen
`static` AVX2-Searcher pro Scan-Stelle (ADR 24 §12b) und `Fn`-Closures im
heißen Pfad. In Nikaia liegt `unsafe` nur in `crates/unsafe/*` (ADR-218).
Bewiesene Prüfungen dürfen entfallen (ADR-306). Der Beweiser erreicht
**keine Strings** (also keine UTF-8-Grenzen) und keine Terminierung
(`formal-verification.md` §2). Lambdas über Zyklen werden zu `&dyn Fn`.

Drei Wege für die `unsafe`-Stellen:

| Weg | Vorteile | Nachteile |
| :--- | :--- | :--- |
| Beweiser auf Text erweitern (Byte-Index ist Zeichengrenze, ASCII-Lauf) | Allgemein: jeder Text-Code in Nikaia profitiert, nicht nur Parser. Die Grenzen sind bewiesen statt vertraut. | Großer Bau am Beweiser. |
| Neue Primitive in `crates/unsafe` (wie `checked-text`, `proven-index`) | Schnell, folgt ADR-218. | Die Vertrauensbasis wächst, unbewiesenes Rust. |
| Prüfungen behalten | Keine Arbeit. | Instruktionskosten im heißesten Pfad (Größe offen, messen). |

Offene Lücken, die erst die Probe zeigt: Gibt es ein Gegenstück zu
`static` pro Aufrufstelle? Wird SWAR auf `u64` mit Überlaufprüfung teuer?
Was kostet `&dyn Fn` gegenüber Monomorphisierung?

**Einordnung:** Lücken plus eine Abwägung (Beweiser vs. `crates/unsafe`).
**Tendenz:** zuerst die Probe aus H3, dann entscheiden.

## H12 Lücken in `std` und Sprache

Aus `nk:docs/selfhosting-gaps.md` und den Abhängigkeiten von winnow-grammar
(lasso, dashmap, ahash, rayon, memchr). Die Liste ist **vorläufig**, erst
eine Portierungsprobe zeigt die vollständige.

| Lücke | Stand | Einordnung |
| :--- | :--- | :--- |
| `BTreeMap::entry`, `BTreeSet::extend` | ADR-collections-1, #950 `ready` | Lücke, priorisieren |
| `mem::take` | ADR-views-1, #949 `ready` | Lücke, priorisieren |
| `array.iter()` | ADR-collections-2, #955 `ready` | Lücke, priorisieren |
| `String::pop`/`insert_str` | #583 | Lücke |
| `m.iter_mut()`/`values_mut()` (NK1307) | **nicht entschieden** (ADR-views-2 §6) | Abwägung, Owner |
| Nebenläufiger Interner/Map (heute lasso `ThreadedRodeo`, dashmap) | unklar, ob `std` das hat (`lock-free.md`, `mutex-floor.md`) | erst prüfen |
| Konfigurierbarer Hasher (ahash) | unklar | erst prüfen |
| Grammatik-Defekte #999, #1001, #1002, #1004, #1039, #1049, #1050 | offen, `ready` | Defekte |

**Allgemeiner Vorteil:** Alles hier hilft auch dem Self-Hosting und den
Paketen. Nachteile gibt es außer bei NK1307 keine.

## H13 Tests

Nach #821 sind neue Tests nur in Nikaia erlaubt. Die 397 Rust-Tests und 13
trybuild-Fälle testen eine Rust-API, die es so nicht mehr gibt (kein
`grammar!`). Sie werden zu `.nika`-Tests mit `.stdout`/`.stderr`; die
Fehlerfälle von trybuild werden NK-Diagnose-Tests. Die Rust-Seite wird
über den Test geprüft, den #978 Schritt 2 ohnehin verlangt (ein Paket aus
einem Rust-Crate benutzt). **Kein Hindernis**, aber ein Arbeitsposten.

---

## Was ich nicht als Hindernis sehe

* **Fehlende Makros:** Mit H1 (Generator im Compiler) braucht es keine.
* **Terminierungsbeweise:** Der Beweiser kann sie nicht, aber winnow-grammar
  hat sie heute auch nicht. Das ist ein möglicher späterer Gewinn, keine
  Bedingung.
* **Streaming und `par_fold` im Lowering** (#928 Schritt 9): Das entfällt,
  wenn der Generator selbst in Nikaia steht und diese Formen direkt erzeugt.

## Vorschlag für die Reihenfolge der Entscheidungen

1. **H4** (Aktionen) und damit, was „winnow-grammar für Rust-Nutzer“ künftig
   ist. Davon hängen H2, H6 und H8 ab.
2. **H7** und die entschiedenen Lücken aus H12 priorisieren. Das sind reine
   Vorteile und hilft yaml sofort.
3. **H1, H2, H8, H9** als eine ADR „Der Grammatik-Generator ist Nikaia“.
4. **H3/H11** nach einer Probe (Laufzeitstück in Nikaia, Instruktionen
   gezählt).
5. **H5, H6, NK1307, `stored-views`** als einzelne Owner-Entscheidungen, wenn
   ein konkreter Bedarf sie dringend macht.
