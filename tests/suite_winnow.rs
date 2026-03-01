macro_rules! test_case {
    ($name:ident, { $($grammar:tt)* }, [ $(($input:expr, $($check:tt)*)),* $(,)? ]) => {
        grammar_kit::test_case_impl!(
            backend: {
                grammar_macro: winnow_grammar::grammar,
                test_trait: winnow_grammar::testing::WinnowTestExt,
                parser_mut: mut
            },
            name: $name,
            grammar: { $($grammar)* },
            cases: [ $( ($input, $($check)*) ),* ]
        );
    };
}

include!("../../core/grammar-kit/src/common_tests/simple.rs");
