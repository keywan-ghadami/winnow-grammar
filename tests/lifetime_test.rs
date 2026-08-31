use winnow::stream::{LocatingSlice, Stateful};
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext};

grammar! {
    grammar TestGrammar {
        // This rule should parse an identifier and return it as a string slice
        // with the same lifetime as the input string.
        pub get_identifier -> &'a str = name:raw_ident -> { name }
    }
}

#[test]
fn test_ident_lifetime() {
    // Use a String to ensure the lifetime is not 'static, which is a more robust test
    let input_string = String::from("hello_world");

    // The winnow parser now expects a ParseContext for state management (e.g., interning),
    // as required by ADR-11. We initialize the stream state accordingly.
    let mut stream = Stateful {
        state: ParseContext::<()>::default(),
        input: LocatingSlice::new(input_string.as_str()),
    };

    let result = TestGrammar::parse_get_identifier().parse_next(&mut stream);
    assert_eq!(result, Ok("hello_world"));
}
