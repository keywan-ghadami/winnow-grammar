#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use syn_grammar_model::parse_grammar;

mod codegen;

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    // 1. Parse & Validate using syn-grammar-model
    let m_ast = match parse_grammar(input.into()) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Generate Code using local winnow codegen
    match codegen::generate_rust(m_ast) {
        Ok(stream) => stream.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[doc(hidden)]
#[proc_macro]
pub fn include_grammar(_input: TokenStream) -> TokenStream {
    quote::quote! {
        compile_error!("include_grammar! is not supported.");
    }.into()
}
