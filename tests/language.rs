use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

#[derive(Debug, PartialEq)]
pub enum Stmt {
    Let(String, Expr),
    Expr(Expr),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Num(u32),
    Add(Box<Expr>, Box<Expr>),
    Var(String),
}

grammar! {
    grammar MiniLang {
        pub stmt -> Stmt =
            "let" name:raw_ident "=" e:expr ";" -> { Stmt::Let(name.to_string(), e) }
          | e:expr ";" -> { Stmt::Expr(e) }

        expr -> Expr =
            l:term "+" r:expr -> { Expr::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }

        term -> Expr =
            n:u32 -> { Expr::Num(n) }
          | i:raw_ident -> { Expr::Var(i.to_string()) }
          | "(" e:expr ")" -> { e }

        pub spanned_term -> (Expr, std::ops::Range<usize>) =
            t:term @ s -> { (t, s) }
    }
}

#[test]
fn test_let_stmt() {
    MiniLang::parse_stmt()
        .parse_test("let x = 1 + 2;")
        .assert_success_is(Stmt::Let(
            "x".to_string(),
            Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2))),
        ));
}

#[test]
fn test_expr_stmt() {
    MiniLang::parse_stmt()
        .parse_test("10 + x;")
        .assert_success_is(Stmt::Expr(Expr::Add(
            Box::new(Expr::Num(10)),
            Box::new(Expr::Var("x".to_string())),
        )));
}

#[test]
fn test_parens() {
    MiniLang::parse_stmt()
        .parse_test("(1 + 2);")
        .assert_success_is(Stmt::Expr(Expr::Add(
            Box::new(Expr::Num(1)),
            Box::new(Expr::Num(2)),
        )));
}

#[test]
fn test_span() {
    MiniLang::parse_spanned_term()
        .parse_test(" 123")
        .assert_success_with(|(expr, span), _state| {
            assert_eq!(expr, &Expr::Num(123));
            assert_eq!(span, &(1..4));
        });
}
