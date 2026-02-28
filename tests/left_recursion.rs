use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Num(u32),
    Add(Box<Expr>, Box<Expr>),
}

grammar! {
    grammar LeftRec {
        pub rule expr -> Expr =
            l:expr "+" r:term -> { Expr::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }

        rule term -> Expr =
            n:u32 -> { Expr::Num(n) }
    }
}

#[test]
fn test_left_recursion() {
    let input = LocatingSlice::new("1 + 2 + 3");
    let result = LeftRec::parse_expr.parse(input).unwrap();
    // (1 + 2) + 3
    assert_eq!(
        result,
        Expr::Add(
            Box::new(Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2)))),
            Box::new(Expr::Num(3))
        )
    );
}
