use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar SpaceParser {
        // Disable automatic whitespace skipping by overriding ws with empty
        use winnow::combinator::empty;
        #[allow(dead_code)]
        rule ws -> () = empty -> { () }

        pub rule test_space0 -> String =
            s:space0 -> { s }
        pub rule test_space1 -> String =
            s:space1 -> { s }
    }
}

#[test]
fn test_space_literal() {
    SpaceParser::parse_test_space0
        .parse_test("   ")
        .assert_success_is("   ".to_string());

    SpaceParser::parse_test_space0
        .parse_test("")
        .assert_success_is("".to_string());

    SpaceParser::parse_test_space1
        .parse_test("   ")
        .assert_success_is("   ".to_string());

    SpaceParser::parse_test_space1
        .parse_test("")
        .assert_failure();
}
