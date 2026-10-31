use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar HexParser {
        pub rule test_hex -> String =
            h:hex_digit1 -> { h.to_string() }
    }
}

#[test]
fn test_hex_literal() {
    HexParser::parse_test_hex()
        .parse_test("1A2b")
        .assert_success_is("1A2b".to_string());

    HexParser::parse_test_hex()
        .parse_test("0")
        .assert_success_is("0".to_string());

    HexParser::parse_test_hex()
        .parse_test("F")
        .assert_success_is("F".to_string());

    HexParser::parse_test_hex().parse_test("g").assert_failure();
}

grammar! {
    grammar HexDigit {
        pub rule one -> char = h:hex_digit -> { h }
        // A run of a character class is the text it matched.
        pub rule run -> &'a str = h:hex_digit* -> { h }
        pub rule escape -> u8 = "\\x" h:hex_digit{2} -> { u8::from_str_radix(h, 16).unwrap() }
    }
}

grammar! {
    grammar OwnHexDigit {
        // A rule of the grammar's own by that name is the one meant.
        hex_digit -> char = "z" -> { 'z' }
        pub rule two -> (char, char) = a:hex_digit b:hex_digit -> { (a, b) }
    }
}

#[test]
fn hex_digit_is_one_hex_digit() {
    for (input, c) in [("0", '0'), ("9", '9'), ("a", 'a'), ("F", 'F')] {
        HexDigit::parse_one().parse_test(input).assert_success_is(c);
    }
    HexDigit::parse_one().parse_test("g").assert_failure();
    HexDigit::parse_one().parse_test("").assert_failure();
    HexDigit::parse_run()
        .parse_test("1A2b")
        .assert_success_is("1A2b");
    HexDigit::parse_run().parse_test("").assert_success_is("");
    HexDigit::parse_escape()
        .parse_test("\\x7F")
        .assert_success_is(0x7F);
    HexDigit::parse_escape().parse_test("\\x7").assert_failure();
}

#[test]
fn a_rule_named_hex_digit_shadows_the_built_in() {
    OwnHexDigit::parse_two()
        .parse_test("zz")
        .assert_success_is(('z', 'z'));
    OwnHexDigit::parse_two().parse_test("ab").assert_failure();
}
