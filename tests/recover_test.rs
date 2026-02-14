use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

#[derive(Debug, PartialEq, Clone)]
pub struct Stmt;

grammar! {
    grammar RecoveryTest {
        pub rule stmt -> Option<Stmt> =
            // If `parse_stmt` fails, skip until `;`
            // `s` will be `Option<Stmt>` (Some if success, None if recovered)
            s:recover(parse_stmt, ";") ";" -> { s }

        rule parse_stmt -> Stmt = "let" "x" -> { Stmt }
    }
}

#[test]
fn test_recovery() {
    // Valid input
    let input = LocatingSlice::new("let x;");
    let result = RecoveryTest::parse_stmt.parse(input).unwrap();
    assert_eq!(result, Some(Stmt));

    // Invalid input (recovery triggered)
    // "let y" fails `parse_stmt` ("let x"), so it skips tokens until ";"
    let input = LocatingSlice::new("let y;");
    let result = RecoveryTest::parse_stmt.parse(input).unwrap();
    assert_eq!(result, None);

    // Invalid input with more garbage
    let input = LocatingSlice::new("garbage data;");
    let result = RecoveryTest::parse_stmt.parse(input).unwrap();
    assert_eq!(result, None);

    // Partial match failure
    let input = LocatingSlice::new("let;");
    let result = RecoveryTest::parse_stmt.parse(input).unwrap();
    assert_eq!(result, None);
}
