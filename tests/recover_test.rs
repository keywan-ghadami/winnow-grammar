use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

grammar! {
    grammar RecoverTest {
        rule item -> i32 = i:i32 ";" -> { i }

        pub rule list -> Vec<Option<i32>> =
            items:recover(item, ";")* -> { items }
    }
}

#[test]
fn test_recovery() {
    let input = LocatingSlice::new("1; 2; bad; 3;");
    let result = RecoverTest::parse_list.parse(input).unwrap();

    assert_eq!(result.len(), 4);
    assert_eq!(result[0], Some(1));
    assert_eq!(result[1], Some(2));
    assert_eq!(result[2], None);
    assert_eq!(result[3], Some(3));
}
