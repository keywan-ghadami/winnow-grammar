use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar Args {
        pub rule main -> i32 =
            "start" v:value(10) -> { v }

        rule value(offset: i32) -> i32 =
            i:integer -> { i + offset }
    }
}

#[test]
fn test_args() {
    let input = "start 5";
    let input = LocatingSlice::new(input);
    let result = Args::parse_main.parse(input).unwrap();
    assert_eq!(result, 15);
}
