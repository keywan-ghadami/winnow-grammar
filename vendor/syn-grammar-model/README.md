# syn-grammar-model

**Shared semantic model and parser for `syn-grammar`.**

This crate provides the core logic for parsing the `grammar! { ... }` DSL, validating it, and transforming it into a semantic model. It is designed to be reusable for different backends (e.g., a `winnow` generator or a documentation generator).

## Architecture

The processing pipeline consists of four stages:

1.  **Parsing (`parser`)**: Converts the raw `TokenStream` into a syntactic AST (`parser::GrammarDefinition`). This handles the concrete syntax of the DSL.
2.  **Transformation (`model`)**: Converts the syntactic AST into a semantic model (`model::GrammarDefinition`). This simplifies the structure (e.g., flattening groups, resolving inheritance placeholders).
3.  **Validation (`validator`)**: Checks the semantic model for errors, such as undefined rules, argument mismatches, or invalid token usage.
4.  **Analysis (`analysis`)**: Provides helper functions to query the model, such as detecting left-recursion, collecting custom keywords, or finding "Cut" operators.

## Usage

If you are building a custom backend for `syn-grammar`, use the pipeline as follows:

```rust,ignore
use syn_grammar_model::{parser, model, validator, analysis};
use syn::parse_macro_input;
use proc_macro::TokenStream;

#[proc_macro]
pub fn my_grammar_backend(input: TokenStream) -> TokenStream {
    // 1. Parse: TokenStream -> Syntactic AST
    let p_ast = parse_macro_input!(input as parser::GrammarDefinition);

    // 2. Transform: Syntactic AST -> Semantic Model
    let m_ast: model::GrammarDefinition = p_ast.into();

    // 3. Validate: Check for semantic errors
    if let Err(e) = validator::validate(&m_ast) {
        return e.to_compile_error().into();
    }

    // 4. Analyze & Generate
    // e.g. Collect keywords to generate a module for them
    let keywords = analysis::collect_custom_keywords(&m_ast);
    
    // ... generate your code ...
    TokenStream::new()
}
```
