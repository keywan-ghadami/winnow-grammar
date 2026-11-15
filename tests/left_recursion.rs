use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

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
    LeftRec::parse_expr()
        .parse_test("1 + 2 + 3")
        .assert_success_is(Expr::Add(
            Box::new(Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2)))),
            Box::new(Expr::Num(3)),
        ));
}

// A loop of left-recursive suffixes backtracks out of the suffix it could not
// finish. The error it backtracked from is the one the input has; the loop
// records it like a discarded alternative.

#[derive(Debug, Clone)]
pub enum Calc {
    Num(i64),
    Add(Box<Calc>, Box<Calc>),
    Mul(Box<Calc>, Box<Calc>),
}

grammar! {
    grammar Arith {
        pub rule expr -> Calc =
            l:expr "+" r:term -> { Calc::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }
        rule term -> Calc =
            l:term "*" r:atom -> { Calc::Mul(Box::new(l), Box::new(r)) }
          | a:atom -> { a }
        rule atom -> Calc =
            n:i64 -> { Calc::Num(n) }
          | paren(e:expr) -> { e }
    }
}

fn arith_error(q: &str) -> String {
    Arith::parse_expr().parse_test(q).inner.unwrap_err()
}

#[test]
fn unfinished_suffix_at_end_of_input() {
    let e = arith_error("2 +");
    assert!(
        e.starts_with(
            "unexpected end of input, expected one of: `(`, integer literal at line 1, column 4"
        ),
        "{e}"
    );
    assert!(e.ends_with("\nin atom\nin term\nin expr"), "{e}");
}

#[test]
fn unfinished_suffix_before_a_token() {
    let e = arith_error("1 + * 2");
    assert!(
        e.starts_with(
            "expected one of: `(`, integer literal; found unexpected token `*` at line 1, column 5"
        ),
        "{e}"
    );
}

#[test]
fn unfinished_suffix_inside_parentheses() {
    let e = arith_error("2 * (1 + )");
    assert!(
        e.starts_with("expected one of: `(`, integer literal; found unexpected token `)` at line 1, column 10"),
        "{e}"
    );
}

/// A suffix that fails at its operator began nothing: leftover input is still
/// reported as such.
#[test]
fn no_suffix_is_leftover_input() {
    let e = arith_error("1 2");
    assert!(
        e.starts_with("expected end of input; found unexpected token `2`"),
        "{e}"
    );
}
