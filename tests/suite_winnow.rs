/*
//now dwfined in testing.rs!
//this mavro definition should be removed in favkr of that defined in testing.rs
//todo use winnow_grammar::test_case
//todo double check if suitenandnsyn grammar needs to be adapted for compatibillity
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
}*/
use winnow_grammar::test_case;
include!("../../core/grammar-kit/src/common_tests/simple.rs");
