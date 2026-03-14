use winnow::Parser;
use winnow_grammar::grammar;

grammar! {
    grammar TestGrammar {
        // This rule should parse an identifier and return it as a string slice
        // with the same lifetime as the input string.
        pub get_identifier -> &'a str = name:ident -> { name.as_ref() }
    }
}

#[test]
fn test_ident_lifetime() {
    let mut input = "hello_world";
    let result = TestGrammar::parse_get_identifier(&mut input);
    assert_eq!(result, Ok("hello_world"));
}
