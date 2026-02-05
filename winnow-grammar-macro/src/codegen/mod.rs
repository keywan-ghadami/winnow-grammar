use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn_grammar_model::model::{GrammarDefinition, Rule};

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let grammar_name = &grammar.name;
    
    // Generate rules
    let rules = grammar.rules.iter().map(generate_rule);

    Ok(quote! {
        pub mod #grammar_name {
            use winnow::prelude::*;
            use winnow::token::literal;
            
            // Re-export testing framework if needed or define specific test helpers
            
            #(#rules)*
        }
    })
}

fn generate_rule(rule: &Rule) -> TokenStream {
    let rule_name = &rule.name;
    let fn_name = format_ident!("parse_{}", rule_name);
    let ret_type = &rule.return_type;

    // TODO: Implement actual pattern matching generation based on rule.variants
    // For now, we generate a stub that fails to compile if used, 
    // forcing implementation of the specific logic.
    
    quote! {
        pub fn #fn_name(input: &mut &str) -> PResult<#ret_type> {
            // Placeholder: This needs to be implemented by traversing rule.variants
            // and converting ModelPattern to winnow combinators (alt, seq, etc.)
            todo!("Implement winnow generation for rule {}", stringify!(#rule_name));
        }
    }
}
