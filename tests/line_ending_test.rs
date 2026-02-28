use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar LineEndingParser {
        // We override ws to do nothing so we can test whitespace sensitive parsers
        // Using "custom_ws" to avoid conflict if any, but rule ws -> () is the standard override.
        // We need to make sure we don't recurse infinitely if ws calls ws.
        // Empty string literal is a parser that consumes nothing and succeeds.
        rule ws -> () = empty -> { () }
        pub rule test_line_ending -> String =
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
