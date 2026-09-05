//! Fluent assertions for parser tests.
//!
//! **Origin:** taken over from `grammar-kit` (`src/testing.rs`) during the move
//! out of the syn-grammar monorepo, fork point `64be1ef` (2026-08-31). The
//! content is backend-neutral - no `syn`, no `proc-macro2` - and was pulled
//! inline so that `winnow-grammar` does not drag in the complete syn runtime as
//! a dependency for the sake of 341 lines.

use std::fmt::{Debug, Display};

// Helper for custom error formatting
type ErrorFormatter<E> = Box<dyn Fn(&E, Option<&str>) -> String>;

/// A `Result` with fluent assertions.
///
/// Keeps ownership of the result so that several assertions can be chained.
/// `S` is an optional state that a backend can pass along; the syn path does
/// not use it and leaves it at `()`.
pub struct TestResult<T, E, S = ()> {
    /// The result under test.
    pub inner: Result<T, E>,
    /// Backend-specific state, if one was passed along.
    pub state: Option<S>,
    /// Free text that appears in every failure output.
    pub context: Option<String>,
    /// The source text that was parsed - for pretty output.
    pub source: Option<String>,
    /// How an error is rendered in the panic output.
    pub formatter: Option<ErrorFormatter<E>>,
}

impl<T: Debug, E: Display + Debug, S> TestResult<T, E, S> {
    /// Lifts a `Result` into a `TestResult` without context and without state.
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

    /// The configured context as a line for the failure output, otherwise empty.
    pub fn format_context(&self) -> String {
        self.context
            .as_ref()
            .map(|c| format!("\nContext:  {}", c))
            .unwrap_or_default()
    }

    /// Renders `err` with the configured formatter, otherwise via `Display`.
    ///
    /// Output only. The `assert_failure_*` assertions deliberately compare
    /// against `Display`, not against the rendered version.
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
    /// Deprecated - use [`assert_failure`](Self::assert_failure).
    pub fn assert_is_err(self) -> E {
        self.assert_failure()
    }

    #[deprecated(
        note = "this method should not be used, if you see this warning this indicates corruption of the test by ai hallucinations"
    )]
    /// Deprecated - use [`assert_success`](Self::assert_success).
    pub fn get_success_value(self) -> T {
        self.assert_success()
    }

    #[deprecated(
        note = "this method should not be used, if you see this warning this indicates corruption of the test by ai hallucinations"
    )]
    /// Deprecated - use [`assert_failure_contains`](Self::assert_failure_contains).
    /// `_code` is ignored.
    pub fn assert_error_contains(self, _code: usize, expected_msg_part: &str) {
        self.assert_failure_contains(expected_msg_part);
    }
}

// Special assertions for float types
impl<E: Display + Debug, S> TestResult<f64, E, S> {
    /// Compares against `expected` with `f64::EPSILON` as tolerance.
    ///
    /// Caution: for magnitudes well above 1.0 this is practically an exact
    /// comparison - `EPSILON` is an absolute, not a relative distance.
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
    /// Compares against `expected` with `f32::EPSILON` as tolerance.
    ///
    /// The same limitation as with the `f64` version.
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

/// Turns any `Result` into a [`TestResult`].
pub trait Testable<T, E> {
    /// Lifts `self` into a `TestResult` in order to attach assertions.
    fn test(self) -> TestResult<T, E>;
}

impl<T: Debug, E: Display + Debug> Testable<T, E> for Result<T, E> {
    fn test(self) -> TestResult<T, E> {
        TestResult::new(self)
    }
}
