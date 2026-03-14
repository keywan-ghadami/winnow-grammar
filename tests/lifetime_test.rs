use winnow::stream::{LocatingSlice, Stateful};
use winnow::Parser;
use winnow_grammar::grammar;

grammar! {
    grammar TestGrammar {
        // This rule should parse an identifier and return it as a string slice
        // with the same lifetime as the input string.
        pub get_identifier -> &str = name:ident -> { name.as_ref() }
    }
}

#[test]
fn test_ident_lifetime() {
    // Use a String to ensure the lifetime is not 'static, which is a more robust test
    let input_string = String::from("hello_world");

    // The winnow parser expects a mutable Stateful stream as input for `parse_next`
    let mut stream = Stateful {
        state: (),
        input: LocatingSlice::new(input_string.as_str()),
    };

    let result = TestGrammar::parse_get_identifier().parse_next(&mut stream);
    assert_eq!(result, Ok("hello_world"));
}
