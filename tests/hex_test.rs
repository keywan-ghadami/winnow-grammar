use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar HexParser {
        pub rule test_hex -> String =
            h:hex_digit1 -> { h }
    }
}

#[test]
fn test_hex_literal() {
    let input = LocatingSlice::new("1A2b");
    let result = HexParser::parse_test_hex.parse(input).unwrap();
    assert_eq!(result, "1A2b");

    let input = LocatingSlice::new("0");
    let result = HexParser::parse_test_hex.parse(input).unwrap();
    assert_eq!(result, "0");

    let input = LocatingSlice::new("F");
    let result = HexParser::parse_test_hex.parse(input).unwrap();
    assert_eq!(result, "F");

    let input = LocatingSlice::new("g");
    let result = HexParser::parse_test_hex.parse(input);
    assert!(result.is_err());
}
