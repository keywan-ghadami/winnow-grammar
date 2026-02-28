use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar OctParser {
        pub rule test_oct -> String =
            o:oct_digit1 -> { o }
    }
}

#[test]
fn test_oct_literal() {
    let input = LocatingSlice::new("1234567");
    let result = OctParser::parse_test_oct.parse(input).unwrap();
    assert_eq!(result, "1234567");

    let input = LocatingSlice::new("0");
    let result = OctParser::parse_test_oct.parse(input).unwrap();
    assert_eq!(result, "0");

    let input = LocatingSlice::new("8");
    let result = OctParser::parse_test_oct.parse(input);
    assert!(result.is_err());
}
