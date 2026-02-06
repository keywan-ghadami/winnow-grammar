// src/lib.rs

// Ensure the macro crate is linked
extern crate winnow_grammar_macro;

// Re-export the macro
pub use winnow_grammar_macro::grammar;

// Re-export winnow so generated code has access to it
pub use winnow;

// Re-export testing utilities from syn-grammar (grammar-kit)
// Note: You might need to implement Testable for winnow::PResult later
pub use syn_grammar::testing;
