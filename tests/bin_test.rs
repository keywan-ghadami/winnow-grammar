use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar BinParser {
        pub rule test_bin -> String =
            b:binary_digit1 -> { b }
    }
}

#[test]
fn test_bin_literal() {
    let input = LocatingSlice::new("10101");
    let result = BinParser::parse_test_bin.parse(input).unwrap();
    assert_eq!(result, "10101");

    let input = LocatingSlice::new("0");
    let result = BinParser::parse_test_bin.parse(input).unwrap();
    assert_eq!(result, "0");

    let input = LocatingSlice::new("1");
    let result = BinParser::parse_test_bin.parse(input).unwrap();
    assert_eq!(result, "1");

    let input = LocatingSlice::new("2");
    let result = BinParser::parse_test_bin.parse(input);
    assert!(result.is_err());
}
