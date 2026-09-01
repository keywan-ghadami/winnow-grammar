//! Fluente Zusicherungen fuer Parser-Tests.
//!
//! **Herkunft:** uebernommen aus `grammar-kit` (`src/testing.rs`) beim Auszug aus
//! dem syn-grammar-Monorepo, Fork-Punkt `64be1ef` (2026-08-31). Der Inhalt ist
//! backend-neutral - kein `syn`, kein `proc-macro2` - und wurde inline gezogen,
//! damit `winnow-grammar` nicht wegen 341 Zeilen die komplette syn-Laufzeit als
//! Abhaengigkeit mitschleppt.

use std::fmt::{Debug, Display};

// Helper for custom error formatting
type ErrorFormatter<E> = Box<dyn Fn(&E, Option<&str>) -> String>;

/// Ein `Result` mit fluenten Zusicherungen.
///
/// Behaelt das Ergebnis in Besitz, damit mehrere Zusicherungen verkettet werden
/// koennen. `S` ist ein optionaler Zustand, den ein Backend mitgeben kann; der
/// syn-Pfad benutzt ihn nicht und laesst ihn auf `()`.
pub struct TestResult<T, E, S = ()> {
    /// Das gepruefte Ergebnis.
    pub inner: Result<T, E>,
    /// Backend-spezifischer Zustand, falls einer mitgegeben wurde.
    pub state: Option<S>,
    /// Freitext, der in jeder Fehlerausgabe mit erscheint.
    pub context: Option<String>,
    /// Der Quelltext, aus dem geparst wurde - fuer die huebsche Ausgabe.
    pub source: Option<String>,
    /// Wie ein Fehler in der Panic-Ausgabe gerendert wird.
    pub formatter: Option<ErrorFormatter<E>>,
}

impl<T: Debug, E: Display + Debug, S> TestResult<T, E, S> {
    /// Hebt ein `Result` in ein `TestResult` ohne Kontext und ohne Zustand.
    pub fn new(result: Result<T, E>) -> Self {
        Self {
            inner: result,
            state: None,
            context: None,
            source: None,
            formatter: None,
        }
    }

    /// Adds a state object to the test result.
    pub fn with_state(mut self, state: S) -> Self {
        self.state = Some(state);
        self
    }

    /// Adds context description to the test result for better failure messages.
    pub fn with_context(mut self, context: &str) -> Self {
        self.context = Some(context.to_string());
        self
    }

    /// Adds the source code string to the test result for pretty printing errors.
    pub fn with_source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Adds a custom error formatter.
    pub fn with_formatter<F>(mut self, formatter: F) -> Self
    where
        F: Fn(&E, Option<&str>) -> String + 'static,
    {
        self.formatter = Some(Box::new(formatter));
        self
    }

    /// Der gesetzte Kontext als Zeile fuer die Fehlerausgabe, sonst leer.
    pub fn format_context(&self) -> String {
        self.context
            .as_ref()
            .map(|c| format!("\nContext:  {}", c))
            .unwrap_or_default()
    }

    /// Rendert `err` mit dem gesetzten Formatter, sonst ueber `Display`.
    ///
    /// Nur fuer die Ausgabe. Die `assert_failure_*`-Zusicherungen vergleichen
    /// bewusst gegen `Display`, nicht gegen die gerenderte Fassung.
    pub fn format_err(&self, err: &E) -> String {
        if let Some(formatter) = &self.formatter {
            formatter(err, self.source.as_deref())
        } else {
            format!("{}", err)
        }
    }

    /// Prints the result to stdout for debugging purposes.
    /// Useful when running tests with `-- --nocapture`.
    pub fn inspect(self) -> Self {
        let ctx = self.format_context();
        match &self.inner {
            Ok(val) => {
                println!("\n🔎 INSPECT SUCCESS: {}\nValue: {:?}\n", ctx, val);
            }
            Err(e) => {
                let msg = self.format_err(e);
                println!(
                    "\n🔎 INSPECT FAILURE: {}\nMessage: {}\nDebug:   {:?}\n",
                    ctx, msg, e
                );
            }
        }
        self
    }

    // =========================================================================
    // Success Assertions
    // =========================================================================

    /// 1. Asserts success and returns the value.
    ///    Terminates the chain for `TestResult` but allows inspecting the value `T`.
    pub fn assert_success(self) -> T {
        let ctx = self.format_context();
        match self.inner {
            Ok(val) => val,
            Err(ref e) => {
                let msg = self.format_err(e);
                panic!(
                    "\n🔴 TEST FAILED (Expected Success, but got Error):{}\n\nError Message:\n{}\n\nError Debug:\n{:?}\n", 
                    ctx, msg, e
                );
            }
        }
    }

    /// 2. Asserts success AND checks the value directly against an expected value.
    pub fn assert_success_is<Exp>(self, expected: Exp) -> T
    where
        T: PartialEq<Exp>,
        Exp: Debug,
    {
        let ctx = self.format_context();
        let val = self.assert_success();

        if val != expected {
            panic!(
                "\n🔴 TEST FAILED (Value Mismatch):{}\nExpected: {:?}\nGot:      {:?}\n",
                ctx, expected, val
            );
        }
        val
    }

    /// 3. Asserts success AND checks the value using a closure.
    pub fn assert_success_with<F>(mut self, f: F) -> T
    where
        F: FnOnce(&T, &S),
        S: Debug,
    {
        let state = self.state.take();
        let val = self.assert_success();
        let state_ref = state
            .as_ref()
            .expect("State must be provided to use assert_success_with");
        f(&val, state_ref);
        val
    }

    /// 4. Asserts success AND checks the Debug representation matches.
    ///    Useful for types where PartialEq is hard to implement (e.g. syn types with Spans).
    pub fn assert_success_debug(self, expected_debug: &str) -> T {
        let ctx = self.format_context();
        let val = self.assert_success();
        let actual_debug = format!("{:?}", val);

        if actual_debug != expected_debug {
            panic!(
                "\n🔴 TEST FAILED (Debug Mismatch):{}\nExpected: {:?}\nGot:      {:?}\n",
                ctx, expected_debug, actual_debug
            );
        }
        val
    }

    /// 7. Asserts success AND checks if the string representation contains a specific substring.
    pub fn assert_success_contains(self, expected_part: &str) -> T
    where
        T: Display,
    {
        let ctx = self.format_context();
        let val = self.assert_success();
        let val_str = val.to_string();

        if !val_str.contains(expected_part) {
            panic!(
                "\n🔴 TEST FAILED (Content Mismatch):{}\nExpected to contain: {:?}\nGot:                 {:?}\n",
                ctx, expected_part, val_str
            );
        }
        val
    }

    // =========================================================================
    // Failure Assertions
    // =========================================================================

    /// 5. Asserts failure and returns the error `E`.
    ///    Terminates the chain so you can manually inspect the error object.
    pub fn assert_failure(self) -> E {
        let ctx = self.format_context();
        match self.inner {
            Ok(val) => {
                panic!(
                    "\n🔴 TEST FAILED (Expected Failure, but got Success):{}\nParsed Value: {:?}\n",
                    ctx, val
                );
            }
            Err(e) => e,
        }
    }

    /// 6. Asserts failure AND checks if the message contains a specific text.
    ///    Returns `Self` to allow chaining multiple assertions on the same error.
    pub fn assert_failure_contains(self, expected_msg_part: &str) -> Self {
        let ctx = self.format_context();

        match &self.inner {
            Ok(val) => {
                panic!(
                    "\n🔴 TEST FAILED (Expected Failure, but got Success):{}\nParsed Value: {:?}\n",
                    ctx, val
                );
            }
            Err(err) => {
                let actual_msg = err.to_string();

                if !actual_msg.contains(expected_msg_part) {
                    let formatted = self.format_err(err);
                    panic!(
                        "\n🔴 TEST FAILED (Error Message Mismatch):{}\nExpected part: {:?}\nActual msg:    {:?}\n\nError Debug:\n{:?}\n\nFormatted:\n{}\n", 
                        ctx, expected_msg_part, actual_msg, err, formatted
                    );
                }
            }
        }
        self
    }

    /// 8. Asserts failure AND checks if the message DOES NOT contain a specific text.
    ///    Returns `Self` to allow chaining.
    pub fn assert_failure_not_contains(self, unexpected_part: &str) -> Self {
        let ctx = self.format_context();

        match &self.inner {
            Ok(val) => {
                panic!(
                    "\n🔴 TEST FAILED (Expected Failure, but got Success):{}\nParsed Value: {:?}\n",
                    ctx, val
                );
            }
            Err(err) => {
                let actual_msg = err.to_string();

                if actual_msg.contains(unexpected_part) {
                    let formatted = self.format_err(err);
                    panic!(
                        "\n🔴 TEST FAILED (Unexpected Error Message Content):{}\nUnexpected part: {:?}\nActual msg:      {:?}\n\nError Debug:\n{:?}\n\nFormatted:\n{}\n", 
                        ctx, unexpected_part, actual_msg, err, formatted
                    );
                }
            }
        }
        self
    }

    // --- Deprecated Aliases ---

    #[deprecated(
        note = "this method should not be used, if you see this warning this indicates corruption of the test by ai hallucinations"
    )]
    /// Veraltet - benutze [`assert_failure`](Self::assert_failure).
    pub fn assert_is_err(self) -> E {
        self.assert_failure()
    }

    #[deprecated(
        note = "this method should not be used, if you see this warning this indicates corruption of the test by ai hallucinations"
    )]
    /// Veraltet - benutze [`assert_success`](Self::assert_success).
    pub fn get_success_value(self) -> T {
        self.assert_success()
    }

    #[deprecated(
        note = "this method should not be used, if you see this warning this indicates corruption of the test by ai hallucinations"
    )]
    /// Veraltet - benutze [`assert_failure_contains`](Self::assert_failure_contains).
    /// `_code` wird ignoriert.
    pub fn assert_error_contains(self, _code: usize, expected_msg_part: &str) {
        self.assert_failure_contains(expected_msg_part);
    }
}

// Special assertions for float types
impl<E: Display + Debug, S> TestResult<f64, E, S> {
    /// Vergleicht gegen `expected` mit `f64::EPSILON` als Toleranz.
    ///
    /// Achtung: fuer Betraege deutlich ueber 1.0 ist das praktisch ein exakter
    /// Vergleich - `EPSILON` ist ein absoluter, kein relativer Abstand.
    pub fn assert_success_approx(self, expected: f64) -> f64 {
        let ctx = self.format_context();
        let val = self.assert_success();

        if (val - expected).abs() > f64::EPSILON {
            panic!(
                "\n🔴 TEST FAILED (Approximate Value Mismatch):{}\nExpected: {:?}\nGot:      {:?}\nDiff:     {:?}\n",
                ctx, expected, val, (val - expected).abs()
            );
        }
        val
    }
}

impl<E: Display + Debug, S> TestResult<f32, E, S> {
    /// Vergleicht gegen `expected` mit `f32::EPSILON` als Toleranz.
    ///
    /// Dieselbe Einschraenkung wie bei der `f64`-Fassung.
    pub fn assert_success_approx(self, expected: f32) -> f32 {
        let ctx = self.format_context();
        let val = self.assert_success();

        if (val - expected).abs() > f32::EPSILON {
            panic!(
                "\n🔴 TEST FAILED (Approximate Value Mismatch):{}\nExpected: {:?}\nGot:      {:?}\nDiff:     {:?}\n",
                ctx, expected, val, (val - expected).abs()
            );
        }
        val
    }
}

/// Macht aus jedem `Result` ein [`TestResult`].
pub trait Testable<T, E> {
    /// Hebt `self` in ein `TestResult`, um Zusicherungen anzuhaengen.
    fn test(self) -> TestResult<T, E>;
}

impl<T: Debug, E: Display + Debug> Testable<T, E> for Result<T, E> {
    fn test(self) -> TestResult<T, E> {
        TestResult::new(self)
    }
}
