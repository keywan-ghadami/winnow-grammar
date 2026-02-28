use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar HexParser {
        pub rule test_hex -> String =
            h:hex_digit1 -> { h }
    }
}

#[test]
fn test_hex_literal() {
    HexParser::parse_test_hex
        .parse_test("1A2b")
        .assert_success_is("1A2b".to_string());

    HexParser::parse_test_hex
        .parse_test("0")
        .assert_success_is("0".to_string());

    HexParser::parse_test_hex
        .parse_test("F")
        .assert_success_is("F".to_string());

    HexParser::parse_test_hex.parse_test("g").assert_failure();
}
