use winnow::error::ContextError;
use winnow::stream::LocatingSlice;
use winnow::Parser;

pub use grammar_kit::testing::*;

/// Extension trait for winnow parsers to simplify testing.
///
/// This trait allows writing tests similar to `syn::parse::Parser::parse_str`.
/// It handles the creation of `LocatingSlice` and conversion of results into `TestResult`.
pub trait WinnowTestExt<'a, O> {
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String>;
}

impl<'a, P, O> WinnowTestExt<'a, O> for P
where
    P: Parser<LocatingSlice<&'a str>, O, ContextError>,
    O: std::fmt::Debug,
{
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String> {
        let stream = LocatingSlice::new(input);
        match self.parse(stream) {
            Ok(val) => TestResult::new(Ok(val)).with_source(input),
            Err(e) => {
                // formatting the error simple for now
                let msg = format!("{}", e);
                TestResult::new(Err(msg)).with_source(input)
            }
        }
    }
}

/// A macro to define a test case using the winnow backend.
#[macro_export]
macro_rules! test_case {
    ($name:ident, rule: $rule:ident, { $($grammar:tt)* }, [ $(($input:expr, $($check:tt)*)),* $(,)? ]) => {
        $crate::testing::test_case_impl!(
            backend: {
                grammar_macro: $crate::grammar,
                test_trait: $crate::testing::WinnowTestExt,
                parser_mut: mut
            },
            name: $name,
            rule: $rule,
            grammar: { $($grammar)* },
            cases: [ $( ($input, $($check)*) ),* ]
        );
    };
    ($name:ident, { $($grammar:tt)* }, [ $(($input:expr, $($check:tt)*)),* $(,)? ]) => {
        $crate::testing::test_case_impl!(
            backend: {
                grammar_macro: $crate::grammar,
                test_trait: $crate::testing::WinnowTestExt,
                parser_mut: mut
            },
            name: $name,
            grammar: { $($grammar)* },
            cases: [ $( ($input, $($check)*) ),* ]
        );
    };
}

// Re-export test_case_impl from grammar_kit so winnow_test_case can use it
pub use grammar_kit::test_case_impl;
