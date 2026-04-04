use winnow_grammar::grammar;
use winnow::Parser;
use winnow::stream::{LocatingSlice, Stateful};
use winnow_grammar::{InternerContext, ParseContext, Symbol};

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
    grammar AdvancedGrammar {
        // From language.rs (MiniLang)
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

        // From literal_bindings.rs (LiteralBindings)
        pub rule literal_binding -> String =
            label:"literal" -> { label.to_string() }

        pub rule optional_literal_binding -> Option<String> =
            label:"literal"? -> { label.map(|s| s.to_string()) }

        pub rule literal_span_binding -> usize =
            "literal" @ span -> { span.end - span.start }

        pub rule literal_binding_with_span -> (String, usize) =
            label:"literal" @ span -> { (label.to_string(), span.end - span.start) }

        // From interning.rs, combined into one grammar
        pub rule regular_ident -> &'a str =
            i:raw_ident -> { i }

        pub rule two_interned_idents -> (Symbol, Symbol) =
            i1:ident i2:ident -> { (i1, i2) }
  }
}

fn main() {
    // --- All tests are now combined into the main function ---

    // Test: Let statement
    let state = ParseContext::<()>::default();
    let input = Stateful { input: LocatingSlice::new("let x = 1 + 2;"), state };
    let result = AdvancedGrammar::parse_stmt().parse(input).unwrap();
    assert_eq!(result, Stmt::Let(
        "x".to_string(),
        Expr::Add(Box::new(Expr::Num(1)), Box::new(Expr::Num(2))),
    ));

    // Test: Expression statement
    let state = ParseContext::<()>::default();
    let input = Stateful { input: LocatingSlice::new("10 + x;"), state };
    let result = AdvancedGrammar::parse_stmt().parse(input).unwrap();
    assert_eq!(result, Stmt::Expr(Expr::Add(
        Box::new(Expr::Num(10)),
        Box::new(Expr::Var("x".to_string())),
    )));

    // Test: Parentheses
    let state = ParseContext::<()>::default();
    let input = Stateful { input: LocatingSlice::new("(1 + 2);"), state };
    let result = AdvancedGrammar::parse_stmt().parse(input).unwrap();
    assert_eq!(result, Stmt::Expr(Expr::Add(
        Box::new(Expr::Num(1)),
        Box::new(Expr::Num(2)),
    )));

    // Test: Span
    let state = ParseContext::<()>::default();
    let mut input = Stateful { input: LocatingSlice::new(" 123"), state };
<<<<<<< HEAD
    let (remaining_input, (expr, span)) = AdvancedGrammar::parse_spanned_term().parse_next(&mut input).unwrap();
    assert_eq!(expr, Expr::Num(123));
    assert_eq!(span, 1..4);
    assert_eq!(remaining_input.input.fragment(), &"");
=======
    let (expr, span) = AdvancedGrammar::parse_spanned_term().parse_next(&mut input).unwrap();
    assert_eq!(expr, Expr::Num(123));
    assert_eq!(span, 1..4);
    assert_eq!(*input.input, "");
>>>>>>> 883176a (interning)

    // Test: Regular identifier (no interning)
    let state = ParseContext::<()>::default();
    let input = Stateful { input: LocatingSlice::new("test"), state };
    let result = AdvancedGrammar::parse_regular_ident().parse(input).unwrap();
    assert_eq!(result, "test");

    // Test: Interned identifiers
    
    // Test with same identifiers
    let state_eq = ParseContext::<()>::default();
    let input_eq = Stateful { input: LocatingSlice::new("hello hello"), state: state_eq };
    let (s1_eq, s2_eq) = AdvancedGrammar::parse_two_interned_idents().parse(input_eq).unwrap();
    assert_eq!(s1_eq, s2_eq, "The same identifier string should result in the same Symbol");

    // Test with different identifiers
    let state_ne = ParseContext::<()>::default();
    let input_ne = Stateful { input: LocatingSlice::new("hello world"), state: state_ne };
    let (s1_ne, s2_ne) = AdvancedGrammar::parse_two_interned_idents().parse(input_ne).unwrap();
    assert_ne!(s1_ne, s2_ne, "Different identifier strings should result in different Symbols");

    // Test: Literal binding
    let input = Stateful { input: LocatingSlice::new("literal"), state: ParseContext::<()>::default() };
    let parsed = AdvancedGrammar::parse_literal_binding().parse(input).unwrap();
    assert_eq!(parsed, "literal");

    // Test: Optional literal binding
    let input = Stateful { input: LocatingSlice::new("literal"), state: ParseContext::<()>::default() };
    let parsed = AdvancedGrammar::parse_optional_literal_binding().parse(input).unwrap();
    assert_eq!(parsed, Some("literal".to_string()));

    let input = Stateful { input: LocatingSlice::new(""), state: ParseContext::<()>::default() };
    let parsed = AdvancedGrammar::parse_optional_literal_binding().parse(input).unwrap();
    assert_eq!(parsed, None);

    // Test: Literal span binding
    let input = Stateful { input: LocatingSlice::new("literal"), state: ParseContext::<()>::default() };
    let parsed = AdvancedGrammar::parse_literal_span_binding().parse(input).unwrap();
    assert_eq!(parsed, 7);

    // Test: Literal binding with span
    let input = Stateful { input: LocatingSlice::new("literal"), state: ParseContext::<()>::default() };
    let parsed = AdvancedGrammar::parse_literal_binding_with_span().parse(input).unwrap();
    assert_eq!(parsed, ("literal".to_string(), 7));

    println!("All advanced tests passed!");
}
