use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

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
    let input = "10 // add 20
 + 20";
    let stream = LocatingSlice::new(input);
    let result = CommentAware::parse_add.parse(stream).unwrap();
    assert_eq!(result, 30);
}
