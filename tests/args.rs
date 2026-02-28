use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Args {
        pub rule main -> i32 =
            "start" v:value(offset=10) -> { v }

        rule value(offset: i32) -> i32 =
            i:i32 -> { i + offset }
    }
}

#[test]
fn test_args() {
    Args::parse_main.parse_test("start 5").assert_success_is(15);
}
