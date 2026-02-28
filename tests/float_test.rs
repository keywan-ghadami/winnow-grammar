use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar FloatParser {
        pub rule test_float -> f64 =
            f:f64 -> { f }
    }
}

#[test]
fn test_float_literal() {
    FloatParser::parse_test_float
        .parse_test("1.5")
        .assert_success_approx(1.5);

    FloatParser::parse_test_float
        .parse_test("-0.5")
        .assert_success_approx(-0.5);

    FloatParser::parse_test_float
        .parse_test("123")
        .assert_success_approx(123.0);

    FloatParser::parse_test_float
        .parse_test("abc")
        .assert_failure();
}
