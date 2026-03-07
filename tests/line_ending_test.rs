use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar LineEndingParser {
        // We override ws to do nothing so we can test whitespace sensitive parsers
        // Using "custom_ws" to avoid conflict if any, but rule ws -> () is the standard override.
        // We need to make sure we don't recurse infinitely if ws calls ws.
        // Empty string literal is a parser that consumes nothing and succeeds.
        #[allow(dead_code)]
        WS = empty
        pub rule test_line_ending -> String =
            s:line_ending -> { s }
    }
}

#[test]
fn test_line_ending_literal() {
    LineEndingParser::parse_test_line_ending
        .parse_test("\n")
        .assert_success_is("\n".to_string());

    LineEndingParser::parse_test_line_ending
        .parse_test("\r\n")
        .assert_success_is("\r\n".to_string());

    LineEndingParser::parse_test_line_ending
        .parse_test("a")
        .assert_failure();
}
