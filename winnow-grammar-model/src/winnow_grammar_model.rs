//! # winnow-grammar-model
//!
//! Parser, Modell und Validator der Grammatik-DSL.
//!
//! **Herkunft:** Fork von `syn-grammar-model` aus
//! <https://github.com/keywan-ghadami/syn-grammar>, abgezweigt bei Commit
//! `64be1ef830e5996935ff6465b9552f2cdd060dff` (2026-08-31, Version 0.9.0-Entwurf).
//! Ab hier entwickeln sich beide Fassungen unabhaengig weiter; fuer einen
//! Abgleich ist der Fork-Punkt der Bezugspunkt. Gegenueber dem Original fehlt
//! `model::types` (`Identifier`/`StringLiteral`/`SpannedValue`) - winnow bringt
//! eigene Laufzeittypen mit.
//!
//! This library contains the shared logic for parsing, validating, and analyzing
//! grammar definitions. It is intended to be used by procedural macros
//! that generate parsers or documentation from the grammar DSL.
//!
//! ## Pipeline
//!
//! 1. **[parser]**: Parse input tokens into a syntactic AST.
//! 2. **[model]**: Convert the AST into a semantic model (via `Into`).
//! 3. **[validator]**: Validate the model for semantic correctness.
//! 4. **[analysis]**: Extract information (keywords, recursion) for code generation.

use proc_macro2::TokenStream;
use syn::Result;

pub mod analysis;
pub mod model;
pub mod parser;
pub mod validator;

pub use model::backend::{Backend, BuiltIn};
pub use proc_macro2::Span;

/// Reusable pipeline: Parses, transforms, and validates the grammar.
///
/// This encapsulates the standard 3-step process used by all backends.
///
/// This function uses the provided `Backend` to validate built-ins.
pub fn parse_grammar<B: Backend>(input: TokenStream) -> Result<model::GrammarDefinition> {
    // 1. Parsing: From TokenStream to syntactic AST
    let p_ast: parser::GrammarDefinition = syn::parse2(input)?;

    // 2. Transformation: From syntactic AST to semantic model
    let m_ast: model::GrammarDefinition = p_ast.into();

    // 3. Validation: Check for semantic errors
    validator::validate::<B>(&m_ast)?;

    Ok(m_ast)
}
