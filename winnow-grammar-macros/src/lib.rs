#![doc = include_str!("../README.md")]

extern crate proc_macro;

use proc_macro::TokenStream;
use winnow_grammar_model::parse_grammar;
use winnow_grammar_model::Backend;
use winnow_grammar_model::BuiltIn;

mod codegen;

struct WinnowBackend;

impl Backend for WinnowBackend {
    fn get_builtins() -> &'static [BuiltIn] {
        &[
            BuiltIn {
                name: "ident",
                return_type: "Symbol",
            },
            BuiltIn {
                name: "raw_ident",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "string",
                return_type: "&'a str",
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
                return_type: "&'a str",
            },
            BuiltIn {
                name: "digit1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "hex_digit0",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "hex_digit1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "oct_digit0",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "oct_digit1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "binary_digit0",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "binary_digit1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "multispace0",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "multispace1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "space0",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "space1",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "line_ending",
                return_type: "&'a str",
            },
            BuiltIn {
                name: "empty",
                return_type: "()",
            },
            BuiltIn {
                name: "eof",
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
    // Note: validator is patched in vendored source to support typed generic params.
    let m_ast = match parse_grammar::<WinnowBackend>(input.into()) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error().into(),
    };

    // 2. Generate Code using local winnow codegen
    match codegen::generate_rust(m_ast) {
        Ok(stream) => {
            if std::env::var("DEBUG_GRAMMAR").is_ok() {
                eprintln!("{}", stream);
            }
            stream.into()
        }
        Err(e) => e.to_compile_error().into(),
    }
}

// --- `with_span` ---
//
// Taken over from `grammar-kit-macros` when moving out of the syn-grammar
// monorepo (fork point `64be1ef`, 2026-08-31). The macro is backend-neutral;
// keeping it here spares `winnow-grammar` a dependency on the syn-side runtime.
// Must be in the crate root - procedural macro entry points are required there.

use quote::quote;
use syn::parse::Parser as _;
use syn::{parse_macro_input, Fields, ItemStruct};

#[proc_macro_attribute]
pub fn with_span(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);

    // 1. Add the span field to the struct
    if let Fields::Named(ref mut fields) = input.fields {
        fields.named.push(
            syn::Field::parse_named
                .parse2(quote! { pub span: std::ops::Range<usize> })
                .expect("Failed to parse span field"),
        );
    } else {
        return syn::Error::new_spanned(
            &input.fields,
            "with_span can only be used on structs with named fields",
        )
        .to_compile_error()
        .into();
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, _where_clause) = input.generics.split_for_impl();

    // Determine the ParsedData type.
    // For now, we assume the user wants a generic implementation or we'd need more info.
    // However, the Trait WithSpan<ParsedData> is generic.
    // We'll implement it for the struct itself as the data source if it matches,
    // but typically it's used to map from a "Raw" version to the "AST" version.

    // A common pattern is: ParsedData is the same struct but without the span.
    // But since the macro modifies the struct, we implement it for 'Self'.

    let expanded = quote! {
        #input

        impl #impl_generics WithSpan<#name #ty_generics> for #name #ty_generics {
            fn with_span(mut parsed_data: Self, span: std::ops::Range<usize>) -> Self {
                parsed_data.span = span;
                parsed_data
            }
        }
    };

    TokenStream::from(expanded)
}
