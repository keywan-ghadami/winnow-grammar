use winnow::prelude::*;
use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Generics {
        // Explicitly type the parameter so we know it produces `T`.
        rule list<T>(item: impl Parser<ParseInput<'a, S>, T, winnow::error::ContextError> -> Vec<T> =
            "[" elements:item* "]" -> { elements }

        pub rule main -> Vec<u32> = l:list(item=u32_parser) -> { l }

        rule u32_parser -> u32 = i:u32 -> { i }
    }
}

#[test]
fn test_generics() {
    Generics::parse_main()
        .parse_test("[ 1 2 3 ]")
        .assert_success_is(vec![1, 2, 3]);
}
