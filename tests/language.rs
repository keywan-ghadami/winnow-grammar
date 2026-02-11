use winnow::prelude::*;
use winnow_grammar::grammar;

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
        rule stmt -> Stmt =
            "let" name:ident "=" e:expr ";" -> { Stmt::Let(name, e) }
          | e:expr ";" -> { Stmt::Expr(e) }

        rule expr -> Expr =
            l:term "+" r:expr -> { Expr::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }

        rule term -> Expr =
            n:uint -> { Expr::Num(n) }
          | i:ident -> { Expr::Var(i) }
          | "(" e:expr ")" -> { e }
    }
}

#[test]
fn test_let_stmt() {
    let input = "let x = 1 + 2;";
    let result = MiniLang::parse_stmt.parse(input).unwrap();
    assert_eq!(
        result,
        Stmt::Let(
            "x".to_string(),
            Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2)))
        )
    );
}

#[test]
fn test_expr_stmt() {
    let input = "10 + x;";
    let result = MiniLang::parse_stmt.parse(input).unwrap();
    assert_eq!(
        result,
        Stmt::Expr(
            Expr::Add(Box::new(Expr::Num(10)), Box::new(Expr::Var("x".to_string())))
        )
    );
}

#[test]
fn test_parens() {
    let input = "(1 + 2);";
    let result = MiniLang::parse_stmt.parse(input).unwrap();
    assert_eq!(
        result,
        Stmt::Expr(
            Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2)))
        )
    );
}
