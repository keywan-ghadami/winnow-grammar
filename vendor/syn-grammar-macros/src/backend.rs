use syn_grammar_model::{Backend, BuiltIn};

pub struct SynBackend;

impl Backend for SynBackend {
    fn get_builtins() -> &'static [BuiltIn] {
        &[
            // Portable Primitives (returning portable types)
            BuiltIn {
                name: "ident",
                return_type: "syn_grammar_model::types::Identifier",
            },
            BuiltIn {
                name: "string",
                return_type: "syn_grammar_model::types::StringLiteral",
            },
            // Primitive Types (returning standard Rust types)
            BuiltIn {
                name: "char",
                return_type: "char",
            },
            BuiltIn {
                name: "bool",
                return_type: "bool",
            },
            // Integers
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
            // Floats
            BuiltIn {
                name: "f32",
                return_type: "f32",
            },
            BuiltIn {
                name: "f64",
                return_type: "f64",
            },
            // Alternative Bases
            BuiltIn {
                name: "hex_literal",
                return_type: "u64",
            },
            BuiltIn {
                name: "oct_literal",
                return_type: "u64",
            },
            BuiltIn {
                name: "bin_literal",
                return_type: "u64",
            },
            // Spanned Primitives (returning SpannedValue<T>)
            BuiltIn {
                name: "spanned_char",
                return_type: "syn_grammar_model::types::SpannedValue<char>",
            },
            BuiltIn {
                name: "spanned_bool",
                return_type: "syn_grammar_model::types::SpannedValue<bool>",
            },
            BuiltIn {
                name: "spanned_i8",
                return_type: "syn_grammar_model::types::SpannedValue<i8>",
            },
            BuiltIn {
                name: "spanned_i16",
                return_type: "syn_grammar_model::types::SpannedValue<i16>",
            },
            BuiltIn {
                name: "spanned_i32",
                return_type: "syn_grammar_model::types::SpannedValue<i32>",
            },
            BuiltIn {
                name: "spanned_i64",
                return_type: "syn_grammar_model::types::SpannedValue<i64>",
            },
            BuiltIn {
                name: "spanned_i128",
                return_type: "syn_grammar_model::types::SpannedValue<i128>",
            },
            BuiltIn {
                name: "spanned_isize",
                return_type: "syn_grammar_model::types::SpannedValue<isize>",
            },
            BuiltIn {
                name: "spanned_u8",
                return_type: "syn_grammar_model::types::SpannedValue<u8>",
            },
            BuiltIn {
                name: "spanned_u16",
                return_type: "syn_grammar_model::types::SpannedValue<u16>",
            },
            BuiltIn {
                name: "spanned_u32",
                return_type: "syn_grammar_model::types::SpannedValue<u32>",
            },
            BuiltIn {
                name: "spanned_u64",
                return_type: "syn_grammar_model::types::SpannedValue<u64>",
            },
            BuiltIn {
                name: "spanned_u128",
                return_type: "syn_grammar_model::types::SpannedValue<u128>",
            },
            BuiltIn {
                name: "spanned_usize",
                return_type: "syn_grammar_model::types::SpannedValue<usize>",
            },
            BuiltIn {
                name: "spanned_f32",
                return_type: "syn_grammar_model::types::SpannedValue<f32>",
            },
            BuiltIn {
                name: "spanned_f64",
                return_type: "syn_grammar_model::types::SpannedValue<f64>",
            },
            // Low-level token filters (currently return syn types or ())
            BuiltIn {
                name: "alpha",
                return_type: "syn::Ident",
            },
            BuiltIn {
                name: "digit",
                return_type: "syn::Ident",
            },
            BuiltIn {
                name: "alphanumeric",
                return_type: "syn::Ident",
            },
            BuiltIn {
                name: "hex_digit",
                return_type: "syn::Ident",
            },
            BuiltIn {
                name: "oct_digit",
                return_type: "syn::Ident",
            },
            BuiltIn {
                name: "any_byte",
                return_type: "syn::LitByte",
            },
            BuiltIn {
                name: "eof",
                return_type: "()",
            },
            BuiltIn {
                name: "whitespace",
                return_type: "()",
            },
            // Syn-Specific Built-ins
            BuiltIn {
                name: "rust_type",
                return_type: "syn::Type",
            },
            BuiltIn {
                name: "rust_block",
                return_type: "syn::Block",
            },
            BuiltIn {
                name: "lit_str",
                return_type: "syn::LitStr",
            },
            BuiltIn {
                name: "lit_int",
                return_type: "syn::LitInt",
            },
            BuiltIn {
                name: "lit_char",
                return_type: "syn::LitChar",
            },
            BuiltIn {
                name: "lit_bool",
                return_type: "syn::LitBool",
            },
            BuiltIn {
                name: "lit_float",
                return_type: "syn::LitFloat",
            },
            BuiltIn {
                name: "outer_attrs",
                return_type: "Vec<syn::Attribute>",
            },
        ]
    }
}
