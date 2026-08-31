# ADR 14: Shared Context Pattern für String Interning

**Status:** Proposed. Mit dem Auszug aus dem syn-grammar-Monorepo (Fork-Punkt
`64be1ef`, 2026-08-31) hierher übernommen — es betraf schon dort ausschließlich
`winnow-grammar`. Die Nummer stammt aus der ADR-Reihe des Ursprungsrepos; dort war
es zuvor als „ADR-11" geführt, was doppelt vergeben war.

**Datum:** 2024-03-15

## 1. Kontext und Problemstellung

Für die Verbesserung von Performance und Speichereffizienz wird ein String-Interning-Mechanismus für das Parsen von Bezeichnern eingeführt, der über die `ident`-Primitive in der Grammatik-DSL verfügbar ist.

Die zentrale architektonische Herausforderung besteht darin, dass der `InternerContext` (die Datenstruktur für das Interning) kein temporäres Implementierungsdetail des Parsers ist. In einer Compiler-Toolchain ist der Interner eine langlebige, zentrale Datenstruktur, die den Lebenszyklus eines einzelnen Parse-Vorgangs überdauern und über mehrere Quelldateien hinweg (potenziell parallel) geteilt werden muss. Der Parsing-Framework benötigt einen robusten und expliziten Weg, um auf diesen vom Benutzer verwalteten, geteilten Kontext zuzugreifen.

## 2. Die Entscheidung: Das "Shared Context Pattern"

Wir führen ein explizites "Shared Context Pattern" ein. Die Verantwortung für die Erstellung und Lebenszeit des `InternerContext` liegt vollständig beim Benutzer des Frameworks. Der Parser greift auf diesen Kontext über eine wohldefinierte Struktur zu.

1.  **Öffentliche `ParseContext`-Struktur:** Das Framework stellt eine neue, öffentliche und generische Struktur bereit.

    ```rust
    // In `winnow-grammar/src/lib.rs`
    pub struct ParseContext<S = ()> {
        pub interner: std::sync::Arc<InternerContext>,
        pub user_state: S,
    }
    ```

2.  **Unbedingte Verwendung im Stream-Zustand:** Alle vom `grammar!`-Makro generierten Parser verwenden **immer und ausnahmslos** den Zustandstyp `winnow::Stateful<&'a str, ParseContext<S>>`. Es gibt keine bedingte Code-Generierung.

3.  **"Outside-In" Benutzer-Workflow:** Der Anwender der Bibliothek steuert den Kontext:
    a. Der Benutzer instanziiert den `InternerContext` einmal für den gesamten Kompilierungsprozess.
    b. Er verpackt ihn in einen `std::sync::Arc`.
    c. Für jeden Parse-Vorgang (z. B. für jede Datei) wird ein `ParseContext` mit einem Klon des `Arc` erstellt und an den Parser übergeben.
    d. Dies ermöglicht zustandsfreies, hoch-performantes und Thread-sicheres Parsen über mehrere Dateien hinweg.

## 3. Implementierungsdetails

### 3.1. Kerndatenstrukturen (`winnow-grammar/src/lib.rs`)

Die folgenden Strukturen werden in der Haupt-Bibliotheksdatei definiert:

```rust
// In winnow-grammar/src/lib.rs

use std::sync::Arc;
use lasso::{ThreadedRodeo, Spur};

pub type Symbol = Spur;
// WICHTIG: ThreadedRodeo anstelle von Rodeo!
pub type InternerContext = ThreadedRodeo; 

pub struct ParseContext<S = ()> {
    // KEIN Mutex! ThreadedRodeo ist intern bereits thread-safe.
    pub interner: Arc<InternerContext>,
    pub user_state: S,
}

impl<S: Default> Default for ParseContext<S> {
    fn default() -> Self {
        Self {
            interner: Arc::new(InternerContext::new()),
            user_state: S::default(),
        }
    }
}

/// Der Input-Stream-Typ für alle Parser, die vom Makro generiert werden.
pub type ParseInput<'a, S = ()> = winnow::Stateful<&'a str, ParseContext<S>>;
```

### 3.2. Makro-Codegenerierung (`winnow-grammar-macros/`)

Die Makro-Logik wird vereinfacht, da keine bedingten Trait-Bounds mehr nötig sind.

1.  **`codegen/rule.rs`**: Die Signaturen der generierten Parser-Funktionen (`parse_<rule_name>`) werden vereinheitlicht.

    ```rust
    // Generierte Signatur in codegen/rule.rs
    pub fn parse_<rule_name><'a, S>(...) -> impl Parser<
        ::winnow_grammar::ParseInput<'a, S>, // Ist jetzt alias für Stateful<..., ParseContext<S>>
        ...,
        ...
    >
    where
        S: 'a + Clone + std::fmt::Debug,
    { ... }
    ```

2.  **`codegen/expr.rs`**: Die Behandlung der `ident`-Primitive greift direkt auf den Interner zu, ohne Sperrmechanismus.

    ```rust
    // Generierter Code für `i:ident` in codegen/expr.rs
    let s: &str = // ... Code zum Parsen des Bezeichner-Strings ...
    let symbol = input.state_mut().interner.get_or_intern(s);
    Ok(symbol)
    ```

### 3.3. Benutzer-Workflow (Beispiel)

Der Code des Endbenutzers bleibt explizit und einfach:

```rust
use std::sync::Arc;
use winnow_grammar::{ParseContext, InternerContext};

fn main() {
    let input_str = "my_identifier";

    // 1. Benutzer erstellt und verwaltet den geteilten, thread-sicheren Interner
    let interner = Arc::new(InternerContext::new());

    // 2. Benutzer erstellt den ParseContext
    let mut context = ParseContext {
        interner: interner.clone(),
        user_state: (), 
    };

    // 3. Parser wird mit explizitem Zustand ausgeführt
    let input = winnow::Stateful::new(input_str, context);
    // ...
}
```

### 3.4. Test-Harness (`winnow-grammar/src/testing.rs`)

Die Test-Helfer werden angepasst, um den `ParseContext` zu verwalten und für Assertions bereitzustellen, was die Validierung von `Symbol`-Werten ermöglicht.

```rust
// Die `assert_success_with`-Funktion wird angepasst, um den Kontext an die Closure zu übergeben
impl<'a, O> TestResult<'a, O> {
    pub fn assert_success_with<F>(self, check: F)
    where
        // Die Closure erhält nun das Ergebnis UND den finalen Zustand (inkl. Interner)
        F: FnOnce(O, &ParseContext),
    {
        // ...
        check(output, &final_state);
        // ...
    }
}
```

## 4. Konsequenzen

### Positiv

*   **Korrekte Architekturausrichtung:** Die Lebenszeit des Interners wird korrekt von der Anwendung verwaltet.
*   **Performantes & Thread-sicheres Sharing:** Die Verwendung von `Arc<ThreadedRodeo>` ermöglicht Lock-freies, hoch-performantes, paralleles Parsen in denselben Interner, was für Multi-Core-Compiler essenziell ist.
*   **Reduzierte Makro-Komplexität:** Die Code-Generierung wird radikal vereinfacht, da keine bedingten Analysen oder Trait-Bounds mehr nötig sind.
*   **Exzellente Testbarkeit:** Unit-Tests können den `ParseContext` explizit erstellen und nach dem Parsen auf den `interner` zugreifen, um `Symbol`-IDs zu validieren.

### Negativ

*   **Explizites Setup durch den Benutzer:** Anwender müssen nun immer einen `ParseContext` erstellen, auch wenn ihre Grammatik die `ident`-Primitive nicht nutzt. Dies ist ein kleiner, aber notwendiger expliziter Schritt, der die korrekte Architektur sicherstellt.

## 5. Abgelehnte Alternativen

### Alternative A: Gekapselter `InternalParseState`
*   **Grund der Ablehnung:** Architektonisch falsch, da der Interner den Parser überleben und geteilt werden muss.

### Alternative B: Bedingte Generierung von Trait-Bounds
*   **Grund der Ablehnung:** Unnötige Verkomplizierung des Makros und "leaky abstraction".

### Alternative C: `Arc<Mutex<Rodeo>>`
*   **Grund der Ablehnung:** Der `Mutex` erzeugt Lock Contention, was den Durchsatz beim parallelen Parsen zerstört. Die Verwendung der nativ-nebenläufigen Datenstruktur `ThreadedRodeo` ist klar überlegen.
'''