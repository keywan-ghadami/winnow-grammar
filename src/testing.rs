use crate::{ParseContext, ParseInput};
use winnow::stream::LocatingSlice;
use winnow::Parser;

pub use crate::test_result::*;

/// Extension trait for winnow parsers to simplify testing.
///
/// This trait allows writing tests similar to `syn::parse::Parser::parse_str`.
/// It handles the creation of `ParseInput` and conversion of results into `TestResult`.
/// This default trait implementation is fixed to the default state `S = ()`.
pub trait WinnowTestExt<'a, O> {
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String, ParseContext<()>>;
}

// This implementation is specifically for parsers that operate with the default empty state `()`.
// This covers the vast majority of use cases and allows the compiler to infer the state type
// without requiring explicit `::<()>` annotations (the "turbofish") at the call site.
impl<'a, P, O> WinnowTestExt<'a, O> for P
where
    P: Parser<ParseInput<'a, ()>, O, ::winnow::error::ContextError>,
    O: std::fmt::Debug,
{
    fn parse_test(&mut self, input: &'a str) -> TestResult<O, String, ParseContext<()>> {
        let state = ParseContext::<()>::default();
        let mut stream = ParseInput {
            input: LocatingSlice::new(input),
            state,
        };

        let result = self.parse_next(&mut stream).map_err(|e| e.to_string());

        let final_context = stream.state;

        TestResult::new(result)
            .with_source(input)
            .with_state(final_context)
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
            // Import `ParseContext` so it's available in the user's `check` closure.
            use $crate::ParseContext;

            paste! {
                grammar! { grammar [<$name _grammar>] { $($grammar)* } }
            }

            #[test]
            fn run() {
                paste!{
                    macro_rules! run_check {
                        ($inp:expr, val $expect:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>]();
                            parser.parse_test($inp).assert_success_is($expect);
                        };
                        ($inp:expr, err $msg:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>]();
                            parser.parse_test($inp).assert_failure_contains($msg);
                        };
                        ($inp:expr, check $closure:expr) => {
                            #[allow(unused_mut)]
                            let $($parser_mut)? parser = [<$name _grammar>]::[<parse_ $rule>]();
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
