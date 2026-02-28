use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar RecoverTest {
        rule item -> i32 = i:i32 ";" -> { i }

        pub rule list -> Vec<Option<i32>> =
            items:recover(item, ";")* -> { items }
    }
}

#[test]
fn test_recovery() {
    RecoverTest::parse_list
        .parse_test("1; 2; bad; 3;")
        .assert_success_is(vec![Some(1), Some(2), None, Some(3)]);
}
