#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn_grammar_model::parse_grammar;

// Include modules
mod backend;
mod codegen;

use backend::SynBackend;

/// The main macro for defining grammars.
///
/// See the [crate-level documentation](https://docs.rs/syn-grammar) for full syntax and usage details.
///
/// # Example
///
/// ```rust,ignore
/// use syn_grammar::grammar;
///
/// grammar! {
///     grammar MyGrammar {
///         rule main -> i32 = "42" -> { 42 }
///     }
/// }
/// ```
#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    // 1-3. Reusable pipeline: Parse, Transform, Validate
    // We convert proc_macro::TokenStream to proc_macro2::TokenStream via .into()
    let m_ast = match parse_grammar::<SynBackend>(input.into()) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error().into(),
    };

    // 4. Code Generation: From model to finished Rust code (codegen.rs)
    match codegen::generate_rust(m_ast) {
        Ok(stream) => stream.into(),           // Successful code
        Err(e) => e.to_compile_error().into(), // Emit generation error as compiler error
    }
}

#[doc(hidden)]
#[proc_macro]
pub fn include_grammar(_input: TokenStream) -> TokenStream {
    quote! {
        compile_error!("External files are removed in v0.2.0. Please move your grammar inline into grammar! { ... }.");
    }.into()
}
