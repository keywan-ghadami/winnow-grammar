#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use syn_grammar_model::parse_grammar_with_builtins;

mod codegen;

const WINNOW_BUILTINS: &[&str] = &[
    "ident",
    "integer",
    "uint",
    "string",
    "char",
    "any", // Added any to support tests/features.rs
    // We might need to add more winnow primitives here as they are encountered
    "alpha1",
    "digit1",
    "hex_digit0",
    "hex_digit1",
    "oct_digit0",
    "oct_digit1",
    "binary_digit0",
    "binary_digit1",
    "multispace0",
    "multispace1",
    "float",
    "space0",
    "space1",
    "line_ending",
    "empty", // Needed for empty rule
];

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    grammar_impl(input)
}

fn grammar_impl(input: TokenStream) -> TokenStream {
    // 1. Parse & Validate using syn-grammar-model with specific built-ins
    let m_ast = match parse_grammar_with_builtins(input.into(), WINNOW_BUILTINS) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Generate Code using local winnow codegen
    match codegen::generate_rust(m_ast) {
        Ok(stream) => stream.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
