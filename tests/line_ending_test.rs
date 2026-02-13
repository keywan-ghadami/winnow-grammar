use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar LineEndingParser {
        use winnow::combinator::empty;
        // Override ws to do nothing using `empty` combinator to avoid recursion loop
        // (literals like "" would implicitly call ws)
        rule ws -> () = empty -> { () }
        rule test_line_ending -> String =
            s:line_ending -> { s }
    }
}

#[test]
fn test_line_ending_literal() {
    let input = LocatingSlice::new("\n");
    let result = LineEndingParser::parse_test_line_ending
        .parse(input)
        .unwrap();
    assert_eq!(result, "\n");

    let input = LocatingSlice::new("\r\n");
    let result = LineEndingParser::parse_test_line_ending
        .parse(input)
        .unwrap();
    assert_eq!(result, "\r\n");

    let input = LocatingSlice::new("a");
    let result = LineEndingParser::parse_test_line_ending.parse(input);
    assert!(result.is_err());
}
