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
        pub rule stmt -> Stmt =
            "let" name:ident "=" e:expr ";" -> { Stmt::Let(name, e) }
          | e:expr ";" -> { Stmt::Expr(e) }

        rule expr -> Expr =
            l:term "+" r:expr -> { Expr::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }

        rule term -> Expr =
            n:u32 -> { Expr::Num(n) }
          | i:ident -> { Expr::Var(i) }
          | "(" e:expr ")" -> { e }

        pub rule spanned_term -> (Expr, std::ops::Range<usize>) =
            t:term @ s -> { (t, s) }
    }
}

#[test]
fn test_let_stmt() {
    MiniLang::parse_stmt
        .parse_test("let x = 1 + 2;")
        .assert_success_is(Stmt::Let(
            "x".to_string(),
            Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2))),
        ));
}

#[test]
fn test_expr_stmt() {
    MiniLang::parse_stmt
        .parse_test("10 + x;")
        .assert_success_is(Stmt::Expr(Expr::Add(
            Box::new(Expr::Num(10)),
            Box::new(Expr::Var("x".to_string())),
        )));
}

#[test]
fn test_parens() {
    MiniLang::parse_stmt
        .parse_test("(1 + 2);")
        .assert_success_is(Stmt::Expr(Expr::Add(
            Box::new(Expr::Num(1)),
            Box::new(Expr::Num(2)),
        )));
}

#[test]
fn test_span() {
    MiniLang::parse_spanned_term
        .parse_test(" 123")
        .assert_success_with(|(expr, span)| {
            assert_eq!(expr, &Expr::Num(123));
            assert_eq!(span, &(0..4));
        });
}
