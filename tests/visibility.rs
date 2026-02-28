use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

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
    let input = LocatingSlice::new("test");
    let result = VisibilityTest::parse_start.parse(input).unwrap();
    assert_eq!(result, "test");
}

// We cannot easily test that `VisibilityTest::parse_private_rule` is NOT accessible
// in a standard `cargo test` run because it would cause a compile error.
// However, if the codegen works, `parse_start` will be able to call `parse_private_rule`
// because they are in the same module.
