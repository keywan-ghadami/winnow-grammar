use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar Generics {
        // Explicitly type the parameter so we know it produces `T`.
        rule list<T>(item: impl Parser<I, T, winnow::error::ContextError>) -> Vec<T> =
            "[" elements:item* "]" -> { elements }

        pub rule main -> Vec<u32> = l:list(u32_parser) -> { l }

        rule u32_parser -> u32 = i:u32 -> { i }
    }
}

#[test]
fn test_generics() {
    let input = "[ 1 2 3 ]";
    let input = LocatingSlice::new(input);

    let result = Generics::parse_main.parse(input).unwrap();
    assert_eq!(result, vec![1, 2, 3]);
}
