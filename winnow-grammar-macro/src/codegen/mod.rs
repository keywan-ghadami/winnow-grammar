use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn_grammar_model::model::{GrammarDefinition, Rule};
use syn_grammar_model::parser::Pattern;

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

    let variants = rule.variants.iter().map(|v| {
        // Inline variant generation since we can't import Variant type easily
        // Assuming v has 'bindings' and 'action'
        let steps: Vec<_> = v.bindings.iter().map(generate_step).collect();
        let action = &v.action;
        
        quote! {
            |input: &mut &str| -> PResult<#ret_type> {
                #(#steps)*
                Ok(#action)
            }
        }
    });

    // If multiple variants, use alt.
    let body = if rule.variants.len() == 1 {
        let v = &rule.variants[0];
        let steps: Vec<_> = v.bindings.iter().map(generate_step).collect();
        let action = &v.action;
        quote! {
            #(#steps)*
            Ok(#action)
        }
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

fn generate_step(pattern: &Pattern) -> TokenStream {
    match pattern {
        // Handle RuleCall (identifiers, builtins, rule references)
        Pattern::RuleCall { binding, rule_name, .. } => {
            // Check for builtins based on rule_name
            let name_str = quote!(#rule_name).to_string();
            let parser = match name_str.as_str() {
                "ident" => quote! { 
                    (ws, winnow::token::take_while(1.., |c: char| c.is_alphanumeric() || c == '_'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                "integer" => quote! {
                    (ws, winnow::ascii::dec_int).map(|(_, i)| i)
                },
                "string" => quote! {
                     (ws, delimited('"', winnow::ascii::escaped(winnow::token::none_of(("\\", "\"")), '\\', winnow::token::one_of(("\\", "\""))), '"'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                _ => {
                    let fn_name = format_ident!("parse_{}", rule_name);
                    quote! { #fn_name }
                }
            };

            match binding {
                Some(name) => quote! {
                    let #name = #parser.parse_next(input)?;
                },
                None => quote! {
                    let _ = #parser.parse_next(input)?;
                }
            }
        }
        
        // Handle Literals (e.g. "fn", "+")
        // Assuming Pattern::Lit(syn::Lit) exists based on errors
        Pattern::Lit(lit) => {
            let s = match lit {
                syn::Lit::Str(s) => s.value(),
                syn::Lit::Char(c) => c.value().to_string(),
                _ => panic!("Unsupported literal type"),
            };
            quote! {
                let _ = (ws, literal(#s)).map(|(_, s)| s).parse_next(input)?;
            }
        }

        // Handle Groups (e.g. (a b | c))
        Pattern::Group(alternatives, _) => {
            // alternatives is Vec<Vec<Pattern>>
            // Outer vec is alt, inner vec is sequence
            let alts = alternatives.iter().map(|seq| {
                let seq_parsers = seq.iter().map(generate_parser_expr);
                quote! {
                    ( #(#seq_parsers),* )
                }
            });
            
            quote! {
                let _ = alt(( #(#alts),* )).parse_next(input)?;
            }
        }

        // Handle Optional (e.g. term?)
        Pattern::Optional(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! {
                let _ = opt(#p).parse_next(input)?;
            }
        }
        
        // Handle Repetitions
        // Guessing variants based on standard syn patterns since RepeatOp was missing
        Pattern::Star(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! {
                let _ = repeat(0.., #p).parse_next(input)?;
            }
        }
        Pattern::Plus(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! {
                let _ = repeat(1.., #p).parse_next(input)?;
            }
        }
        Pattern::Question(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! {
                let _ = opt(#p).parse_next(input)?;
            }
        }

        _ => quote! {
            compile_error!("Unsupported pattern type in generate_step");
        }
    }
}

// Helper to generate just the parser expression (without 'let _ = ...')
// Used for nesting inside combinators
fn generate_parser_expr(pattern: &Pattern) -> TokenStream {
    match pattern {
        Pattern::RuleCall { rule_name, .. } => {
            let name_str = quote!(#rule_name).to_string();
            match name_str.as_str() {
                "ident" => quote! { 
                    (ws, winnow::token::take_while(1.., |c: char| c.is_alphanumeric() || c == '_'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                "integer" => quote! {
                    (ws, winnow::ascii::dec_int).map(|(_, i)| i)
                },
                "string" => quote! {
                     (ws, delimited('"', winnow::ascii::escaped(winnow::token::none_of(("\\", "\"")), '\\', winnow::token::one_of(("\\", "\""))), '"'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                _ => {
                    let fn_name = format_ident!("parse_{}", rule_name);
                    quote! { #fn_name }
                }
            }
        }
        Pattern::Lit(lit) => {
            let s = match lit {
                syn::Lit::Str(s) => s.value(),
                syn::Lit::Char(c) => c.value().to_string(),
                _ => panic!("Unsupported literal type"),
            };
            quote! {
                (ws, literal(#s)).map(|(_, s)| s)
            }
        }
        Pattern::Group(alternatives, _) => {
            let alts = alternatives.iter().map(|seq| {
                let seq_parsers = seq.iter().map(generate_parser_expr);
                // If sequence has multiple items, tuple combinator
                if seq.len() == 1 {
                    quote! { #(#seq_parsers)* }
                } else {
                    quote! { ( #(#seq_parsers),* ) }
                }
            });
            quote! {
                alt(( #(#alts),* ))
            }
        }
        Pattern::Optional(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! { opt(#p) }
        }
        Pattern::Star(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! { repeat(0.., #p) }
        }
        Pattern::Plus(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! { repeat(1.., #p) }
        }
        Pattern::Question(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! { opt(#p) }
        }
        _ => quote! {
            compile_error!("Unsupported pattern type in generate_parser_expr")
        }
    }
}
