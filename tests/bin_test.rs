use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar BinParser {
        pub rule test_bin -> String =
            b:binary_digit1 -> { b.to_string() }
    }
}

#[test]
fn test_bin_literal() {
    BinParser::parse_test_bin()
        .parse_test("10101")
        .assert_success_is("10101".to_string());

    BinParser::parse_test_bin()
        .parse_test("0")
        .assert_success_is("0".to_string());

    BinParser::parse_test_bin()
        .parse_test("1")
        .assert_success_is("1".to_string());

    BinParser::parse_test_bin().parse_test("2").assert_failure();
}
