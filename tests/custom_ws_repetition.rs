use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar CustomWs {
        // Override ws to match underscore
        rule ws -> () = "_" -> { () }

        pub rule list -> Vec<u32> = l:u32+ -> { l }
    }
}

#[test]
fn test_custom_ws_repetition() {
    CustomWs::parse_list
        .parse_test("1_2_3")
        .assert_success_is(vec![1, 2, 3]);
}
