use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar WsRepro {
        rule ws -> () = multispace0 -> { () }
        pub rule test -> String = "a" -> { "a".to_string() }
        pub rule test_eof -> String = "a" eof -> { "a".to_string() }
    }
}

#[test]
fn test_ws_recursion() {
    let input = LocatingSlice::new("  a");
    let result = WsRepro::parse_test.parse(input);
    assert!(result.is_ok());
}

#[test]
fn test_eof() {
    let input = LocatingSlice::new("a");
    let result = WsRepro::parse_test_eof.parse(input);
    assert!(result.is_ok());

    let input = LocatingSlice::new("a ");
    let result = WsRepro::parse_test_eof.parse(input);
    // Since eof is used, "a " should fail because there is a trailing space if not consumed.
    // Wait, "a" consumes leading ws. Then "eof" runs.
    // If input is "a ", "a" consumes nothing (it matches "a").
    // Then eof runs on " ". It should fail.
    assert!(result.is_err());
}
