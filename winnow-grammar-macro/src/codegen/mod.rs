use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn_grammar_model::model::{GrammarDefinition, Rule};
use syn_grammar_model::parser::{Variant, Binding, Pattern, RepeatOp};

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let grammar_name = &grammar.name;
    
    // Generate rules
    let rules = grammar.rules.iter().map(generate_rule);

    Ok(quote! {
        pub mod #grammar_name {
            use winnow::prelude::*;
            use winnow::token::literal;
            use winnow::combinator::{alt, repeat, opt, delimited};
            
            // Whitespace handling (similar to syn)
            #[allow(dead_code)]
            fn ws(input: &mut &str) -> PResult<()> {
                winnow::ascii::multispace0.void().parse_next(input)
            }

            // Re-export testing framework if needed or define specific test helpers
            
            #(#rules)*
        }
    })
}

fn generate_rule(rule: &Rule) -> TokenStream {
    let rule_name = &rule.name;
    let fn_name = format_ident!("parse_{}", rule_name);
    let ret_type = &rule.return_type;

    let variants = rule.variants.iter().map(|v| generate_variant(v, ret_type));

    // If multiple variants, use alt.
    let body = if rule.variants.len() == 1 {
        let v = &rule.variants[0];
        generate_variant_body(v)
    } else {
        quote! {
            alt((
                #(#variants),*
            )).parse_next(input)
        }
    };

    quote! {
        pub fn #fn_name(input: &mut &str) -> PResult<#ret_type> {
            #body
        }
    }
}

fn generate_variant(variant: &Variant, ret_type: &syn::Type) -> TokenStream {
    let body = generate_variant_body(variant);
    quote! {
        |input: &mut &str| -> PResult<#ret_type> {
            #body
        }
    }
}

fn generate_variant_body(variant: &Variant) -> TokenStream {
    let steps: Vec<_> = variant.bindings.iter().map(generate_binding).collect();
    let action = &variant.action;
    
    quote! {
        #(#steps)*
        Ok(#action)
    }
}

fn generate_binding(binding: &Binding) -> TokenStream {
    let pattern_parser = generate_pattern(&binding.pattern);
    match &binding.name {
        Some(name) => quote! {
            let #name = #pattern_parser.parse_next(input)?;
        },
        None => quote! {
            let _ = #pattern_parser.parse_next(input)?;
        }
    }
}

fn generate_pattern(pattern: &Pattern) -> TokenStream {
    match pattern {
        Pattern::Literal(lit) => {
            let s = lit.value();
            quote! {
                (ws, literal(#s)).map(|(_, s)| s)
            }
        }
        Pattern::RuleCall(name) => {
            let fn_name = format_ident!("parse_{}", name);
            quote! { #fn_name }
        }
        Pattern::Builtin(name) => {
            let n = name.to_string();
            match n.as_str() {
                "ident" => quote! { 
                    (ws, winnow::token::take_while(1.., |c: char| c.is_alphanumeric() || c == '_'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                "integer" => quote! {
                    (ws, winnow::ascii::dec_int).map(|(_, i)| i)
                },
                "string" => quote! {
                    // Basic string literal parser
                     (ws, delimited('"', winnow::ascii::escaped(winnow::token::none_of(("\\", "\"")), '\\', winnow::token::one_of(("\\", "\""))), '"'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                _ => quote! {
                    compile_error!(concat!("Unknown builtin parser: ", #n))
                }
            }
        }
        Pattern::Group(inner) => {
            generate_pattern(inner)
        }
        Pattern::Optional(inner) => {
            let p = generate_pattern(inner);
            quote! { opt(#p) }
        }
        Pattern::Repeat(inner, op) => {
            let p = generate_pattern(inner);
            match op {
                RepeatOp::Star => quote! { repeat(0.., #p) },
                RepeatOp::Plus => quote! { repeat(1.., #p) },
                RepeatOp::Question => quote! { opt(#p) },
            }
        }
        // Fallback for unknown patterns
        _ => quote! {
            compile_error!("Unsupported pattern type")
        }
    }
}
