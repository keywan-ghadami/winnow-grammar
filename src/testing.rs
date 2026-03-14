use crate::ParseInput;
use winnow::error::ContextError;
use winnow::stream::{LocatingSlice, Stateful};
use winnow::Parser;

pub use grammar_kit::testing::*;

/// Extension trait for winnow parsers to simplify testing.
///
/// This trait allows writing tests similar to `syn::parse::Parser::parse_str`.
/// It handles the creation of `ParseInput` and conversion of results into `TestResult`.
pub trait WinnowTestExt<'a, O> {
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String>;
}

// NOTE: This impl is intentionally broad to support grammars that are generic over the state `S`.
// It requires `S` to have a `Default` implementation to create an initial state for testing.
// The `Debug` requirement on `S` comes from the `grammar!` macro itself.
impl<'a, P, O> WinnowTestExt<'a, O> for P
where
    P: Parser<ParseInput<'a, ()>, O, ContextError>,
    O: std::fmt::Debug,
{
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String> {
        let stream = Stateful {
            state: (),
            input: LocatingSlice::new(input),
        };
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
        $crate::test_case_impl! (
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
        $crate::test_case_impl! (
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

/// Implementation of the test case logic.
///
/// This macro creates an outer module named after the test for isolation.
///
/// The entry point defaults to `parse_main` but can be customized via the `rule` parameter.
#[macro_export]
macro_rules! test_case_impl {
    // Variant with explicit rule name
    (
        backend: {
            grammar_macro: $grammar_macro:path,
            test_trait: $test_trait:path,
            parser_mut: $($parser_mut:ident)?
        },
        name: $name:ident,
        rule: $rule:ident,
        grammar: { $($grammar:tt)* },
        cases: [ $( ($input:expr, $($check:tt)*) ),* $(,)? ]
    ) => {
        #[allow(non_snake_case)]
        mod $name {
            use paste::paste;
            use $grammar_macro as grammar;
            use $test_trait;

            paste! {
                grammar! { grammar [<$name _grammar>] { $($grammar)* } }
            }

            #[test]
            fn run() {
                paste!{
                    macro_rules! run_check {
                        ($inp:expr, val $expect:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>];
                            parser.parse_test($inp).assert_success_is($expect);
                        };
                        ($inp:expr, err $msg:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>];
                            parser.parse_test($inp).assert_failure_contains($msg);
                        };
                        ($inp:expr, check $closure:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>];
                            parser.parse_test($inp).assert_success_with($closure);
                        };
                    }
                    $(
                        run_check!($input, $($check)*);
                    )*
                }
            }
        }
    };

    // Variant without explicit rule name (defaults to 'main')
    (
        backend: {
            grammar_macro: $grammar_macro:path,
            test_trait: $test_trait:path,
            parser_mut: $($parser_mut:ident)?
        },
        name: $name:ident,
        grammar: { $($grammar:tt)* },
        cases: [ $( ($input:expr, $($check:tt)*) ),* $(,)? ]
    ) => {
        $crate::test_case_impl! {
            backend: {
                grammar_macro: $grammar_macro,
                test_trait: $test_trait,
                parser_mut: $($parser_mut)?
            },
            name: $name,
            rule: main, // Default rule name
            grammar: { $($grammar)* },
            cases: [ $(($input, $($check)*)),* ]
        }
    };
}

