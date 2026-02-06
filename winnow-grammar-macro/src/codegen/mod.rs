use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn_grammar_model::model::{GrammarDefinition, Rule, ModelPattern};

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let grammar_name = &grammar.name;
    
    // Generate rules
    let rules = grammar.rules.iter().map(generate_rule);

    Ok(quote! {
        pub mod #grammar_name {
            #![allow(unused_imports)]
            // Import types from parent module (e.g. AST structs)
            use super::*;
            
            use winnow::prelude::*;
            use winnow::token::literal;
            use winnow::combinator::{alt, repeat, opt, delimited};
            
            // Whitespace handling (similar to syn)
            #[allow(dead_code)]
            fn ws(input: &mut &str) -> PResult<()> {
                winnow::ascii::multispace0.parse_next(input).map(|_| ())
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
        // RuleVariant has 'pattern' (Vec<ModelPattern>) and 'action' (Expr)
        let steps: Vec<TokenStream> = v.pattern.iter().map(|p| generate_step(p)).collect();
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
        let steps: Vec<TokenStream> = v.pattern.iter().map(|p| generate_step(p)).collect();
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

fn generate_step(pattern: &ModelPattern) -> TokenStream {
    match pattern {
        // Handle RuleCall (identifiers, builtins, rule references)
        ModelPattern::RuleCall { binding, rule_name, .. } => {
            // Check for builtins based on rule_name
            let name_str = rule_name.to_string();
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
        ModelPattern::Lit(lit_str) => {
            let s = lit_str.value();
            quote! {
                let _ = (ws, literal(#s)).map(|(_, s)| s).parse_next(input)?;
            }
        }

        // Handle Groups (e.g. (a b | c))
        ModelPattern::Group(alternatives, _span) => {
            // alternatives is Vec<Vec<ModelPattern>>
            // Outer vec is alt, inner vec is sequence
            let alts: Vec<TokenStream> = alternatives.iter().map(|seq: &Vec<ModelPattern>| {
                let seq_parsers: Vec<TokenStream> = seq.iter().map(|p| generate_parser_expr(p)).collect();
                quote! {
                    ( #(#seq_parsers),* )
                }
            }).collect();
            
            let parser = quote! {
                alt(( #(#alts),* ))
            };

            // Try to find a binding on the inner element if possible
            let binding = get_inner_binding(pattern);

            match binding {
                Some(name) => quote! {
                    let #name = #parser.parse_next(input)?;
                },
                None => quote! {
                    let _ = #parser.parse_next(input)?;
                }
            }
        }

        // Handle Optional (e.g. term?)
        ModelPattern::Optional(inner, _span) => {
            let p = generate_parser_expr(inner);
            let parser = quote! { opt(#p) };
            
            let binding = get_inner_binding(inner);

            match binding {
                Some(name) => quote! {
                    let #name = #parser.parse_next(input)?;
                },
                None => quote! {
                    let _ = #parser.parse_next(input)?;
                }
            }
        }
        
        // Handle Repetitions
        // ModelPattern::Repeat(inner, op)
        ModelPattern::Repeat(inner, _span) => {
             let p = generate_parser_expr(inner);
             // Check if inner has a binding that we should use for the list
             let binding = get_inner_binding(inner);
             
             match binding {
                 Some(name) => quote! { let #name = repeat(0.., #p).parse_next(input)?; },
                 None => quote! { let _ = repeat(0.., #p).parse_next(input)?; }
             }
        }

        // Handle Plus (e.g. term+)
        ModelPattern::Plus(inner, _span) => {
             let p = generate_parser_expr(inner);
             let binding = get_inner_binding(inner);
             
             match binding {
                 Some(name) => quote! { let #name = repeat(1.., #p).parse_next(input)?; },
                 None => quote! { let _ = repeat(1.., #p).parse_next(input)?; }
             }
        }

        // Handle Delimiters
        ModelPattern::Parenthesized(inner, _span) => generate_delimited_step(inner, "(", ")"),
        ModelPattern::Bracketed(inner, _span) => generate_delimited_step(inner, "[", "]"),
        ModelPattern::Braced(inner, _span) => generate_delimited_step(inner, "{", "}"),

        _ => quote! {
            compile_error!("Unsupported pattern type in generate_step");
        }
    }
}

fn generate_delimited_step(inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
    let steps: Vec<TokenStream> = inner.iter().map(|p| generate_step(p)).collect();
    quote! {
        let _ = (ws, literal(#open)).parse_next(input)?;
        #(#steps)*
        let _ = (ws, literal(#close)).parse_next(input)?;
    }
}

// Helper to extract binding from a pattern if it exists.
// Used for Repeat/Optional to lift the binding from inner element.
fn get_inner_binding(pattern: &ModelPattern) -> Option<&syn::Ident> {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => binding.as_ref(),
        // For Group, if it's a single item, we might be able to lift the binding
        ModelPattern::Group(alts, _) => {
            if alts.len() == 1 && alts[0].len() == 1 {
                get_inner_binding(&alts[0][0])
            } else {
                None
            }
        },
        ModelPattern::Optional(inner, _) => get_inner_binding(inner),
        ModelPattern::Repeat(inner, _) => get_inner_binding(inner),
        ModelPattern::Plus(inner, _) => get_inner_binding(inner),
        ModelPattern::Parenthesized(inner, _) 
        | ModelPattern::Bracketed(inner, _) 
        | ModelPattern::Braced(inner, _) => {
            if inner.len() == 1 {
                get_inner_binding(&inner[0])
            } else {
                None
            }
        },
        _ => None,
    }
}

// Helper to generate just the parser expression (without 'let _ = ...')
// Used for nesting inside combinators
fn generate_parser_expr(pattern: &ModelPattern) -> TokenStream {
    match pattern {
        ModelPattern::RuleCall { rule_name, .. } => {
            let name_str = rule_name.to_string();
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
        ModelPattern::Lit(lit_str) => {
            let s = lit_str.value();
            quote! {
                (ws, literal(#s)).map(|(_, s)| s)
            }
        }
        ModelPattern::Group(alternatives, _) => {
            let alts: Vec<TokenStream> = alternatives.iter().map(|seq: &Vec<ModelPattern>| {
                let seq_parsers: Vec<TokenStream> = seq.iter().map(|p| generate_parser_expr(p)).collect();
                // If sequence has multiple items, tuple combinator
                if seq.len() == 1 {
                    quote! { #(#seq_parsers)* }
                } else {
                    quote! { ( #(#seq_parsers),* ) }
                }
            }).collect();
            quote! {
                alt(( #(#alts),* ))
            }
        }
        ModelPattern::Optional(inner, _) => {
            let p = generate_parser_expr(inner);
            quote! { opt(#p) }
        }
        ModelPattern::Repeat(inner, _span) => {
            let p = generate_parser_expr(inner);
            // Assuming Repeat is *
            quote! { repeat(0.., #p) }
        }
        ModelPattern::Plus(inner, _span) => {
            let p = generate_parser_expr(inner);
            quote! { repeat(1.., #p) }
        }
        ModelPattern::Parenthesized(inner, _) => generate_delimited_expr(inner, "(", ")"),
        ModelPattern::Bracketed(inner, _) => generate_delimited_expr(inner, "[", "]"),
        ModelPattern::Braced(inner, _) => generate_delimited_expr(inner, "{", "}"),
        _ => quote! {
            compile_error!("Unsupported pattern type in generate_parser_expr")
        }
    }
}

fn generate_delimited_expr(inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
    let seq_parsers: Vec<TokenStream> = inner.iter().map(|p| generate_parser_expr(p)).collect();
    let inner_parser = if seq_parsers.len() == 1 {
        quote! { #(#seq_parsers)* }
    } else {
        quote! { ( #(#seq_parsers),* ) }
    };
    
    quote! {
        delimited((ws, literal(#open)), #inner_parser, (ws, literal(#close)))
    }
}
