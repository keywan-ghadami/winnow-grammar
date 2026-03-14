use winnow::prelude::*;
use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar CommentAware {

        WSE = multispace1
        WS = (WSE | COMMENT)*
        COMMENT = "//" until(line_ending)

        pub add -> i32 =
            a:i32 "+" b:i32
            -> { a + b }
    }
}

#[test]
fn test_comment_aware() {
    // The parser will ignore the comment and the newline.
    let input = "10 // add 20\n + 20";
    let mut parser = CommentAware::parse_add();
    parser.parse_test(input).assert_success_is(30);
}
