use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar CharParser {
        pub rule test_char -> char =
            c:char -> { c }
    }
}

#[test]
fn test_char_literal() {
    CharParser::parse_test_char
        .parse_test("'a'")
        .assert_success_is('a');

    CharParser::parse_test_char
        .parse_test("'\\n'")
        .assert_success_is('\n');

    CharParser::parse_test_char
        .parse_test("'\\''")
        .assert_success_is('\'');

    CharParser::parse_test_char
        .parse_test("'\\\\'")
        .assert_success_is('\\');
}
