/// Describes a built-in grammar rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIn {
    /// The name of the built-in rule (e.g., "ident", "string").
    pub name: &'static str,
    /// The Rust return type of the built-in rule as a string.
    /// This allows backends to declare portable types (e.g., "syn_grammar_model::model::types::Identifier")
    /// or backend-specific types (e.g., "syn::Ident").
    pub return_type: &'static str,
}

/// A trait that backends must implement to declare their capabilities.
pub trait Backend {
    /// Returns the list of built-in rules supported by this backend.
    fn get_builtins() -> &'static [BuiltIn];
}
