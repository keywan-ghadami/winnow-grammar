#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use syn_grammar_model::parse_grammar;
use syn_grammar_model::Backend;
use syn_grammar_model::BuiltIn;

mod codegen;

struct WinnowBackend;

impl Backend for WinnowBackend {
    fn get_builtins() -> &'static [BuiltIn] {
        &[
            BuiltIn {
                name: "ident",
                return_type: "syn_grammar::Identifier",
            },
            BuiltIn {
                name: "integer",
                return_type: "i32",
            },
            BuiltIn {
                name: "uint",
                return_type: "u32",
            },
            BuiltIn {
                name: "string",
                return_type: "syn_grammar::StringLiteral",
            },
            BuiltIn {
                name: "char",
                return_type: "char",
            },
            BuiltIn {
                name: "any",
                return_type: "char",
            },
            BuiltIn {
                name: "alpha1",
                return_type: "String",
            },
            BuiltIn {
                name: "digit1",
                return_type: "String",
            },
            BuiltIn {
                name: "hex_digit0",
                return_type: "String",
            },
            BuiltIn {
                name: "hex_digit1",
                return_type: "String",
            },
            BuiltIn {
                name: "oct_digit0",
                return_type: "String",
            },
            BuiltIn {
                name: "oct_digit1",
                return_type: "String",
            },
            BuiltIn {
                name: "binary_digit0",
                return_type: "String",
            },
            BuiltIn {
                name: "binary_digit1",
                return_type: "String",
            },
            BuiltIn {
                name: "multispace0",
                return_type: "String",
            },
            BuiltIn {
                name: "multispace1",
                return_type: "String",
            },
            BuiltIn {
                name: "float",
                return_type: "f64",
            },
            BuiltIn {
                name: "space0",
                return_type: "String",
            },
            BuiltIn {
                name: "space1",
                return_type: "String",
            },
            BuiltIn {
                name: "line_ending",
                return_type: "String",
            },
            BuiltIn {
                name: "empty",
                return_type: "()",
            },
        ]
    }
}

#[proc_macro]
pub fn grammar(input: TokenStream) -> TokenStream {
    grammar_impl(input)
}

fn grammar_impl(input: TokenStream) -> TokenStream {
    // 1. Parse & Validate using syn-grammar-model with specific built-ins
    let m_ast = match parse_grammar::<WinnowBackend>(input.into()) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Generate Code using local winnow codegen
    match codegen::generate_rust(m_ast) {
        Ok(stream) => stream.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
