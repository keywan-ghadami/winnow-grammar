use winnow::prelude::*;
use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Generics {
        list<T>(item) -> Vec<T> =
            elements:item* -> { elements }
        pub main -> Vec<u32> = l:list<u32>(u32) -> { l }
    }
}

#[test]
fn test_generics() {
    Generics::parse_main()
        .parse_test("1 2 3")
        .assert_success_is(vec![1, 2, 3]);
}
