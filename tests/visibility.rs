use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar VisibilityTest {
        // Public rule, should be accessible
        pub rule start -> String =
            p:private_rule -> { p }

        // Private rule, should NOT be accessible directly (but we can't easily test compile fail here without trybuild)
        // We will test that it IS generated and callable by `start`.
        rule private_rule -> String =
            "test" -> { "test".to_string() }
    }
}

#[test]
fn test_visibility() {
    VisibilityTest::parse_start()
        .parse_test("test")
        .assert_success_is("test".to_string());
}
