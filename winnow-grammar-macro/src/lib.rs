#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::{TokenStream, TokenTree, Group, Delimiter};
use syn::{parse_macro_input, LitStr};
use syn_grammar_model::parse_grammar;
use std::iter::FromIterator;

mod codegen;

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    grammar_impl(input)
}

fn grammar_impl(input: TokenStream) -> TokenStream {
    // 0. Inject builtins to satisfy validator
    let input = inject_builtins(input);

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

#[proc_macro]
pub fn include_grammar(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    let path_str = input.value();
    
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let path = std::path::Path::new(&manifest_dir).join(path_str);
    
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return syn::Error::new(input.span(), format!("Failed to read grammar file '{}': {}", path.display(), e)).to_compile_error().into(),
    };
    
    let ts: TokenStream = match content.parse() {
        Ok(t) => t,
        Err(e) => return syn::Error::new(input.span(), format!("Failed to tokenize grammar file: {}", e)).to_compile_error().into(),
    };
    
    grammar_impl(ts)
}

fn inject_builtins(input: TokenStream) -> TokenStream {
    let tokens = input.into_iter();
    let mut out_tokens = Vec::new();
    
    for token in tokens {
        if let TokenTree::Group(group) = &token {
            if group.delimiter() == Delimiter::Brace {
                let mut content = group.stream().into_iter().collect::<Vec<_>>();
                
                // Inject dummy rules for builtins so syn-grammar-model doesn't complain about undefined rules.
                // These rules will be filtered out during codegen.
                let dummy_source = "
                    rule uint -> u32 = \"__BUILTIN__\" -> { 0 }
                    rule integer -> i32 = \"__BUILTIN__\" -> { 0 }
                    rule ident -> String = \"__BUILTIN__\" -> { String::new() }
                    rule string -> String = \"__BUILTIN__\" -> { String::new() }
                ";
                
                if let Ok(dummy_ts) = dummy_source.parse::<TokenStream>() {
                     content.extend(dummy_ts);
                }
                
                let mut new_group = Group::new(Delimiter::Brace, TokenStream::from_iter(content));
                new_group.set_span(group.span());
                out_tokens.push(TokenTree::Group(new_group));
            } else {
                out_tokens.push(TokenTree::Group(group.clone()));
            }
        } else {
            out_tokens.push(token);
        }
    }
    TokenStream::from_iter(out_tokens)
}
