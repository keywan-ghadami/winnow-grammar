use proc_macro2::{TokenStream, Span};
use quote::{format_ident, quote, quote_spanned};
use syn_grammar_model::{
    analysis,
    model::{GrammarDefinition, Rule, ModelPattern, RuleVariant},
};

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let grammar_name = &grammar.name;
    let span = Span::mixed_site();
    
    // Generate rules, filtering out builtins we injected
    let rules = grammar.rules.iter()
        .filter(|r| !is_builtin(&r.name.to_string()))
        .map(generate_rule);

    Ok(quote_spanned! {span=>
        #[allow(non_snake_case)]
        pub mod #grammar_name {
            #![allow(unused_imports)]
            #![allow(dead_code)]
            
            // Import types from parent module (e.g. AST structs)
            use super::*;
            
            use ::winnow::prelude::*;
            use ::winnow::token::literal;
            use ::winnow::combinator::{alt, repeat, opt, delimited};
            
            // Whitespace handling (similar to syn)
            #[allow(dead_code)]
            fn ws(input: &mut &str) -> ModalResult<()> {
                ::winnow::ascii::multispace0.parse_next(input).map(|_| ())
            }

            // Re-export testing framework if needed or define specific test helpers
            
            #(#rules)*
        }
    })
}

fn is_builtin(name: &str) -> bool {
    matches!(name, "uint" | "integer" | "ident" | "string")
}

fn generate_rule(rule: &Rule) -> TokenStream {
    let rule_name = &rule.name;
    let span = Span::mixed_site();
    let fn_name = format_ident!("parse_{}", rule_name, span = span);
    let ret_type = &rule.return_type;

    // Check for direct left recursion
    let (recursive_refs, base_refs) = analysis::split_left_recursive(&rule.name, &rule.variants);

    // Create identifier for left-hand side accumulator with consistent span
    let lhs_ident = format_ident!("lhs", span = span);

    let body = if recursive_refs.is_empty() {
        // Standard generation
        generate_variants_body(&rule.variants, ret_type)
    } else {
        // Left-recursive generation
        if base_refs.is_empty() {
            quote_spanned! {span=>
                compile_error!("Left-recursive rule requires at least one non-recursive base variant.")
            }
        } else {
            let base_owned: Vec<RuleVariant> = base_refs.into_iter().cloned().collect();
            let recursive_owned: Vec<RuleVariant> = recursive_refs.into_iter().cloned().collect();

            let base_parser = generate_variants_body(&base_owned, ret_type);
            let loop_body = generate_recursive_loop_body(&recursive_owned, ret_type, &lhs_ident);

            quote_spanned! {span=>
                // 1. Parse Base
                let mut #lhs_ident = #base_parser?;
                
                // 2. Loop to consume recursive tails
                loop {
                    #loop_body
                    break;
                }
                Ok(#lhs_ident)
            }
        }
    };

    quote_spanned! {span=>
        pub fn #fn_name(input: &mut &str) -> ModalResult<#ret_type> {
            #body
        }
    }
}

fn generate_variants_body(variants: &[RuleVariant], ret_type: &syn::Type) -> TokenStream {
    let span = Span::mixed_site();
    let variant_parsers = variants.iter().map(|v| {
        let steps: Vec<TokenStream> = v.pattern.iter().map(generate_step).collect();
        let action = &v.action;
        quote_spanned! {span=>
            |input: &mut &str| -> ModalResult<#ret_type> {
                #(#steps)*
                Ok(#action)
            }
        }
    });

    if variants.len() == 1 {
        let v = &variants[0];
        let steps: Vec<TokenStream> = v.pattern.iter().map(generate_step).collect();
        let action = &v.action;
        quote_spanned! {span=>
            {
                #(#steps)*
                Ok(#action)
            }
        }
    } else {
        quote_spanned! {span=>
            alt((
                #(#variant_parsers),*
            )).parse_next(input)
        }
    }
}

fn generate_recursive_loop_body(variants: &[RuleVariant], ret_type: &syn::Type, lhs_ident: &syn::Ident) -> TokenStream {
    let span = Span::mixed_site();
    
    let arms = variants.iter().map(|v| {
        // The first pattern is the recursive call (e.g. `l:expression`).
        // We skip it for parsing, but we need its binding name to inject `lhs`.
        let lhs_binding = match &v.pattern[0] {
            ModelPattern::RuleCall { binding: Some(b), .. } => Some(b),
            _ => None,
        };

        let bind_lhs = if let Some(b) = lhs_binding {
            quote! { let #b = #lhs_ident.clone(); }
        } else {
            quote! {}
        };

        // Generate steps for the *rest* of the pattern (the tail)
        let tail_steps: Vec<TokenStream> = v.pattern.iter().skip(1).map(generate_step).collect();
        let action = &v.action;

        // We construct an imperative attempt block.
        // We use `input.checkpoint()` to backtrack if the tail fails.
        quote_spanned! {span=>
            {
                let checkpoint = ::winnow::stream::Stream::checkpoint(input);
                let attempt = (|| -> ModalResult<#ret_type> {
                    #(#tail_steps)*
                    #bind_lhs
                    Ok(#action)
                })();

                match attempt {
                    Ok(val) => {
                        #lhs_ident = val;
                        continue;
                    },
                    Err(e) => {
                        // If it's a recoverable error (Backtrack), reset and try next.
                        // If it's a failure (Cut), we should probably propagate, but for now
                        // we treat it as "variant didn't match" unless we implement cut logic.
                        // Winnow's `alt` backtracks on standard errors.
                        ::winnow::stream::Stream::reset(input, &checkpoint);
                    }
                }
            }
        }
    });

    quote_spanned! {span=>
        #(#arms)*
    }
}

fn generate_step(pattern: &ModelPattern) -> TokenStream {
    let span = Span::mixed_site();
    match pattern {
        // Handle RuleCall (identifiers, builtins, rule references)
        ModelPattern::RuleCall { binding, rule_name, .. } => {
            // Check for builtins based on rule_name
            let name_str = rule_name.to_string();
            let parser = match name_str.as_str() {
                "ident" => quote_spanned! {span=> 
                    (ws, ::winnow::token::take_while(1.., |c: char| c.is_alphanumeric() || c == '_'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                "integer" => quote_spanned! {span=>
                    (ws, ::winnow::ascii::dec_int::<_, i32, _>).map(|(_, i)| i)
                },
                "uint" => quote_spanned! {span=>
                    (ws, ::winnow::ascii::dec_uint::<_, u32, _>).map(|(_, i)| i)
                },
                "string" => quote_spanned! {span=>
                     (ws, delimited(
                        '"', 
                        ::winnow::ascii::take_escaped(
                            ::winnow::token::none_of(['\\', '"']), 
                            '\\', 
                            ::winnow::token::one_of(['\\', '"'])
                        ), 
                        '"'
                    ))
                    .map(|(_, s): (_, &str)| s.to_string())
                },
                _ => {
                    let fn_name = format_ident!("parse_{}", rule_name, span = span);
                    quote_spanned! {span=> #fn_name }
                }
            };

            match binding {
                Some(name) => quote_spanned! {span=>
                    let #name = #parser.parse_next(input)?;
                },
                None => quote_spanned! {span=>
                    let _ = #parser.parse_next(input)?;
                }
            }
        }
        
        // Handle Literals (e.g. "fn", "+")
        ModelPattern::Lit(lit_str) => {
            let s = lit_str.value();
            quote_spanned! {span=>
                let _ = (ws, literal(#s)).map(|(_, s)| s).parse_next(input)?;
            }
        }

        // Handle Groups (e.g. (a b | c))
        ModelPattern::Group(alternatives, _span) => {
            // alternatives is Vec<Vec<ModelPattern>>
            // Outer vec is alt, inner vec is sequence
            let alts: Vec<TokenStream> = alternatives.iter().map(|seq: &Vec<ModelPattern>| {
                let seq_parsers: Vec<TokenStream> = seq.iter().map(generate_parser_expr).collect();
                quote_spanned! {span=>
                    ( #(#seq_parsers),* )
                }
            }).collect();
            
            let parser = quote_spanned! {span=>
                alt(( #(#alts),* ))
            };

            // Try to find a binding on the inner element if possible
            let binding = get_inner_binding(pattern);

            match binding {
                Some(name) => quote_spanned! {span=>
                    let #name = #parser.parse_next(input)?;
                },
                None => quote_spanned! {span=>
                    let _ = #parser.parse_next(input)?;
                }
            }
        }

        // Handle Optional (e.g. term?)
        ModelPattern::Optional(inner, _span) => {
            let p = generate_parser_expr(inner);
            let parser = quote_spanned! {span=> opt(#p) };
            
            let binding = get_inner_binding(inner);

            match binding {
                Some(name) => quote_spanned! {span=>
                    let #name = #parser.parse_next(input)?;
                },
                None => quote_spanned! {span=>
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
                 Some(name) => quote_spanned! {span=> let #name: Vec<_> = repeat(0.., #p).parse_next(input)?; },
                 None => quote_spanned! {span=> let _: Vec<_> = repeat(0.., #p).parse_next(input)?; }
             }
        }

        // Handle Plus (e.g. term+)
        ModelPattern::Plus(inner, _span) => {
             let p = generate_parser_expr(inner);
             let binding = get_inner_binding(inner);
             
             match binding {
                 Some(name) => quote_spanned! {span=> let #name: Vec<_> = repeat(1.., #p).parse_next(input)?; },
                 None => quote_spanned! {span=> let _: Vec<_> = repeat(1.., #p).parse_next(input)?; }
             }
        }

        // Handle Delimiters
        ModelPattern::Parenthesized(inner, _span) => generate_delimited_step(inner, "(", ")"),
        ModelPattern::Bracketed(inner, _span) => generate_delimited_step(inner, "[", "]"),
        ModelPattern::Braced(inner, _span) => generate_delimited_step(inner, "{", "}"),

        _ => quote_spanned! {span=>
            compile_error!("Unsupported pattern type in generate_step");
        }
    }
}

fn generate_delimited_step(inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
    let span = Span::mixed_site();
    let steps: Vec<TokenStream> = inner.iter().map(generate_step).collect();
    quote_spanned! {span=>
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
    let span = Span::mixed_site();
    match pattern {
        ModelPattern::RuleCall { rule_name, .. } => {
            let name_str = rule_name.to_string();
            match name_str.as_str() {
                "ident" => quote_spanned! {span=> 
                    (ws, ::winnow::token::take_while(1.., |c: char| c.is_alphanumeric() || c == '_'))
                        .map(|(_, s): (_, &str)| s.to_string())
                },
                "integer" => quote_spanned! {span=>
                    (ws, ::winnow::ascii::dec_int::<_, i32, _>).map(|(_, i)| i)
                },
                "uint" => quote_spanned! {span=>
                    (ws, ::winnow::ascii::dec_uint::<_, u32, _>).map(|(_, i)| i)
                },
                "string" => quote_spanned! {span=>
                     (ws, delimited(
                        '"', 
                        ::winnow::ascii::take_escaped(
                            ::winnow::token::none_of(['\\', '"']), 
                            '\\', 
                            ::winnow::token::one_of(['\\', '"'])
                        ), 
                        '"'
                    ))
                    .map(|(_, s): (_, &str)| s.to_string())
                },
                _ => {
                    let fn_name = format_ident!("parse_{}", rule_name, span = span);
                    quote_spanned! {span=> #fn_name }
                }
            }
        }
        ModelPattern::Lit(lit_str) => {
            let s = lit_str.value();
            quote_spanned! {span=>
                (ws, literal(#s)).map(|(_, s)| s)
            }
        }
        ModelPattern::Group(alternatives, _) => {
            let alts: Vec<TokenStream> = alternatives.iter().map(|seq: &Vec<ModelPattern>| {
                let seq_parsers: Vec<TokenStream> = seq.iter().map(generate_parser_expr).collect();
                // If sequence has multiple items, tuple combinator
                if seq.len() == 1 {
                    quote_spanned! {span=> #(#seq_parsers)* }
                } else {
                    quote_spanned! {span=> ( #(#seq_parsers),* ) }
                }
            }).collect();
            quote_spanned! {span=>
                alt(( #(#alts),* ))
            }
        }
        ModelPattern::Optional(inner, _) => {
            let p = generate_parser_expr(inner);
            quote_spanned! {span=> opt(#p) }
        }
        ModelPattern::Repeat(inner, _span) => {
            let p = generate_parser_expr(inner);
            // Assuming Repeat is *
            quote_spanned! {span=> repeat(0.., #p) }
        }
        ModelPattern::Plus(inner, _span) => {
            let p = generate_parser_expr(inner);
            quote_spanned! {span=> repeat(1.., #p) }
        }
        ModelPattern::Parenthesized(inner, _) => generate_delimited_expr(inner, "(", ")"),
        ModelPattern::Bracketed(inner, _) => generate_delimited_expr(inner, "[", "]"),
        ModelPattern::Braced(inner, _) => generate_delimited_expr(inner, "{", "}"),
        _ => quote_spanned! {span=>
            compile_error!("Unsupported pattern type in generate_parser_expr")
        }
    }
}

fn generate_delimited_expr(inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
    let span = Span::mixed_site();
    let seq_parsers: Vec<TokenStream> = inner.iter().map(generate_parser_expr).collect();
    let inner_parser = if seq_parsers.len() == 1 {
        quote_spanned! {span=> #(#seq_parsers)* }
    } else {
        quote_spanned! {span=> ( #(#seq_parsers),* ) }
    };
    
    quote_spanned! {span=>
        delimited((ws, literal(#open)), #inner_parser, (ws, literal(#close)))
    }
}
