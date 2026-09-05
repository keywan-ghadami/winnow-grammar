use winnow_grammar::grammar;
use winnow_grammar::Symbol;

// Minimal struct definitions to allow macro expansion
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

// The grammar definition, isolated for expansion
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
            i1:ident " " i2:ident -> { (i1, i2) }
    }
}

// Dummy main to make it a valid program for cargo expand
fn main() {}
