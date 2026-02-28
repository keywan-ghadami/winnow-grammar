use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar OctParser {
        pub rule test_oct -> String =
            o:oct_digit1 -> { o }
    }
}

#[test]
fn test_oct_literal() {
    OctParser::parse_test_oct
        .parse_test("1234567")
        .assert_success_is("1234567".to_string());

    OctParser::parse_test_oct
        .parse_test("0")
        .assert_success_is("0".to_string());

    OctParser::parse_test_oct.parse_test("8").assert_failure();
}
