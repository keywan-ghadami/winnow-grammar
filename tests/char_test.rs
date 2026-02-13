use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar CharParser {
        rule test_char -> char =
            c:char -> { c }
    }
}

#[test]
fn test_char_literal() {
    let input = LocatingSlice::new("'a'");
    let result = CharParser::parse_test_char.parse(input).unwrap();
    assert_eq!(result, 'a');

    let input = LocatingSlice::new("'\\n'");
    let result = CharParser::parse_test_char.parse(input).unwrap();
    assert_eq!(result, '\n');

    let input = LocatingSlice::new("'\\''");
    let result = CharParser::parse_test_char.parse(input).unwrap();
    assert_eq!(result, '\'');

    let input = LocatingSlice::new("'\\\\'");
    let result = CharParser::parse_test_char.parse(input).unwrap();
    assert_eq!(result, '\\');
}
