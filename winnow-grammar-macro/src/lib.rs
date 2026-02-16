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
                return_type: "String",
            },
            BuiltIn {
                name: "string",
                return_type: "String",
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
            // Explicit Rust Types
            BuiltIn {
                name: "u8",
                return_type: "u8",
            },
            BuiltIn {
                name: "u16",
                return_type: "u16",
            },
            BuiltIn {
                name: "u32",
                return_type: "u32",
            },
            BuiltIn {
                name: "u64",
                return_type: "u64",
            },
            BuiltIn {
                name: "u128",
                return_type: "u128",
            },
            BuiltIn {
                name: "usize",
                return_type: "usize",
            },
            BuiltIn {
                name: "i8",
                return_type: "i8",
            },
            BuiltIn {
                name: "i16",
                return_type: "i16",
            },
            BuiltIn {
                name: "i32",
                return_type: "i32",
            },
            BuiltIn {
                name: "i64",
                return_type: "i64",
            },
            BuiltIn {
                name: "i128",
                return_type: "i128",
            },
            BuiltIn {
                name: "isize",
                return_type: "isize",
            },
            BuiltIn {
                name: "f32",
                return_type: "f32",
            },
            BuiltIn {
                name: "f64",
                return_type: "f64",
            },
            BuiltIn {
                name: "bool",
                return_type: "bool",
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
