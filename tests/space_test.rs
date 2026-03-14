use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar SpaceParser {
        // Disable automatic whitespace skipping by overriding ws with empty
        use winnow::combinator::empty;
        #[allow(dead_code)]
        WS = empty

        pub test_space0 -> String =
            s:space0 -> { s.to_string() }
        pub test_space1 -> String =
            s:space1 -> { s.to_string() }
    }
}

#[test]
fn test_space_literal() {
    SpaceParser::parse_test_space0()
        .parse_test("   ")
        .assert_success_is("   ".to_string());

    SpaceParser::parse_test_space0()
        .parse_test("")
        .assert_success_is("".to_string());

    SpaceParser::parse_test_space1()
        .parse_test("   ")
        .assert_success_is("   ".to_string());

    SpaceParser::parse_test_space1()
        .parse_test("")
        .assert_failure();
}
