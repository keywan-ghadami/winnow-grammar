use proc_macro2::{TokenStream, Span};
use quote::{format_ident, quote, quote_spanned};
use syn_grammar_model::{
    analysis,
    model::{GrammarDefinition, Rule, ModelPattern, RuleVariant},
};
use std::collections::HashSet;

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let mut codegen = Codegen::new(&grammar);
    codegen.generate()
}

struct Codegen<'a> {
    grammar: &'a GrammarDefinition,
    user_rules: HashSet<String>,
}

impl<'a> Codegen<'a> {
    fn new(grammar: &'a GrammarDefinition) -> Self {
        let user_rules = grammar.rules.iter()
            .map(|r| r.name.to_string())
            .collect();
        Self { grammar, user_rules }
    }

    fn generate(&mut self) -> syn::Result<TokenStream> {
        let grammar_name = &self.grammar.name;
        let span = Span::mixed_site();
        let use_statements = &self.grammar.uses;
        
        let has_user_ws = self.user_rules.contains("ws");

        let rules = self.grammar.rules.iter()
            .map(|r| self.generate_rule(r));

        let use_super = quote_spanned! {Span::call_site()=> use super::*; };

        let ws_parser = if has_user_ws {
            quote_spanned! {span=>
                #[allow(unused_imports)]
                use parse_ws as ws;
            }
        } else {
            quote_spanned! {span=>
                // Whitespace handling (similar to syn)
                #[allow(dead_code)]
                fn ws<I>(input: &mut I) -> ModalResult<()>
                where 
                    I: ::winnow::stream::Stream + ::winnow::stream::StreamIsPartial + for<'a> ::winnow::stream::Compare<&'a str>,
                    <I as ::winnow::stream::Stream>::Token: ::winnow::stream::AsChar + Clone,
                    <I as ::winnow::stream::Stream>::Slice: ::winnow::stream::AsBStr,
                {
                    ::winnow::ascii::multispace0.parse_next(input).map(|_| ())
                }
            }
        };

        Ok(quote_spanned! {span=>
            #[allow(non_snake_case)]
            pub mod #grammar_name {
                #![allow(unused_imports)]
                #![allow(dead_code)]
                
                // Import types from parent module (e.g. AST structs)
                #use_super

                // User-defined use statements
                #(#use_statements)*
                
                use ::winnow::prelude::*;
                use ::winnow::token::literal;
                use ::winnow::combinator::{alt, repeat, opt, delimited};
                
                #ws_parser

                #(#rules)*
            }
        })
    }

    fn generate_rule(&self, rule: &Rule) -> TokenStream {
        let rule_name = &rule.name;
        let rule_name_str = rule_name.to_string();
        let span = Span::mixed_site();
        let fn_name = format_ident!("parse_{}", rule_name, span = span);
        let ret_type = &rule.return_type;

        let params: Vec<TokenStream> = rule.params.iter().map(|(name, ty)| {
            quote! { #name: #ty }
        }).collect();
        
        let (recursive_refs, base_refs) = analysis::split_left_recursive(&rule.name, &rule.variants);

        let lhs_ident = format_ident!("lhs", span = span);

        let body = if recursive_refs.is_empty() {
            self.generate_variants_body(&rule.variants, ret_type)
        } else if base_refs.is_empty() {
            quote_spanned! {span=>
                compile_error!("Left-recursive rule requires at least one non-recursive base variant.")
            }
        } else {
            let base_owned: Vec<RuleVariant> = base_refs.into_iter().cloned().collect();
            let recursive_owned: Vec<RuleVariant> = recursive_refs.into_iter().cloned().collect();

            let base_parser = self.generate_variants_body(&base_owned, ret_type);
            let loop_body = self.generate_recursive_loop_body(&recursive_owned, ret_type, &lhs_ident);

            quote_spanned! {span=>
                let mut #lhs_ident = #base_parser?;
                loop {
                    #loop_body
                    break;
                }
                Ok(#lhs_ident)
            }
        };

        quote_spanned! {span=>
            pub fn #fn_name<I>(input: &mut I, #(#params),*) -> ModalResult<#ret_type>
            where
                I: ::winnow::stream::Stream + ::winnow::stream::StreamIsPartial + ::winnow::stream::Location + for<'a> ::winnow::stream::Compare<&'a str>,
                <I as ::winnow::stream::Stream>::Token: ::winnow::stream::AsChar + Clone,
                <I as ::winnow::stream::Stream>::Slice: ::winnow::stream::AsBStr,
            {
                use ::winnow::Parser;
                use ::winnow::error::ContextError;

                (|input: &mut I| -> ModalResult<#ret_type> {
                    #body
                })
                .context(::winnow::error::StrContext::Label(#rule_name_str))
                .parse_next(input)
            }
        }
    }

    fn generate_variants_body(&self, variants: &[RuleVariant], ret_type: &syn::Type) -> TokenStream {
        let span = Span::mixed_site();
        let variant_parsers = variants.iter().map(|v| {
            let steps: Vec<TokenStream> = v.pattern.iter().map(|p| self.generate_step(p)).collect();
            let action = &v.action;
            quote_spanned! {span=>
                |input: &mut I| -> ModalResult<#ret_type> {
                    #(#steps)*
                    Ok(#action)
                }
            }
        });

        if variants.len() == 1 {
            let v = &variants[0];
            let steps: Vec<TokenStream> = v.pattern.iter().map(|p| self.generate_step(p)).collect();
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

    fn generate_recursive_loop_body(&self, variants: &[RuleVariant], ret_type: &syn::Type, lhs_ident: &syn::Ident) -> TokenStream {
        let span = Span::mixed_site();
        
        let arms = variants.iter().map(|v| {
            let lhs_binding = match &v.pattern[0] {
                ModelPattern::RuleCall { binding: Some(b), .. } => Some(b),
                _ => None,
            };

            let bind_lhs = if let Some(b) = lhs_binding {
                quote! { let #b = #lhs_ident.clone(); }
            } else {
                quote! {}
            };

            let tail_steps: Vec<TokenStream> = v.pattern.iter().skip(1).map(|p| self.generate_step(p)).collect();
            let action = &v.action;

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

    fn generate_step(&self, pattern: &ModelPattern) -> TokenStream {
        let span = Span::mixed_site();
        match pattern {
            ModelPattern::SpanBinding(inner, span_var, _span) => {
                let parser = self.generate_parser_expr(inner);
                let binding = get_inner_binding(inner);
                
                match binding {
                    Some(name) => quote_spanned! {span=>
                        let (#name, #span_var) = #parser.with_span().parse_next(input)?;
                    },
                    None => quote_spanned! {span=>
                        let (_, #span_var) = #parser.with_span().parse_next(input)?;
                    }
                }
            }
            ModelPattern::RuleCall { binding, rule_name, args } => {
                let parser = self.generate_rule_call_parser(rule_name, args);

                match binding {
                    Some(name) => quote_spanned! {span=>
                        let #name = #parser.parse_next(input)?;
                    },
                    None => quote_spanned! {span=>
                        let _ = #parser.parse_next(input)?;
                    }
                }
            }
            ModelPattern::Lit(lit_str) => {
                let s = lit_str.value();
                quote_spanned! {span=>
                    let _ = (ws, literal(#s)).map(|(_, s)| s).parse_next(input)?;
                }
            }
            ModelPattern::Group(alternatives, _span) => {
                if alternatives.len() == 1 {
                    let seq = &alternatives[0];
                    let steps: Vec<TokenStream> = seq.iter().map(|p| self.generate_step(p)).collect();
                    quote_spanned! {span=>
                        #(#steps)*
                    }
                } else {
                     let parser = self.generate_parser_expr(pattern);
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
            }
            ModelPattern::Optional(inner, _span) => {
                let p = self.generate_parser_expr(inner);
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
            ModelPattern::Repeat(inner, _span) => {
                 let p = self.generate_parser_expr(inner);
                 let binding = get_inner_binding(inner);
                 match binding {
                     Some(name) => quote_spanned! {span=> let #name: Vec<_> = repeat(0.., #p).parse_next(input)?; },
                     None => quote_spanned! {span=> let _: Vec<_> = repeat(0.., #p).parse_next(input)?; }
                 }
            }
            ModelPattern::Plus(inner, _span) => {
                 let p = self.generate_parser_expr(inner);
                 let binding = get_inner_binding(inner);
                 match binding {
                     Some(name) => quote_spanned! {span=> let #name: Vec<_> = repeat(1.., #p).parse_next(input)?; },
                     None => quote_spanned! {span=> let _: Vec<_> = repeat(1.., #p).parse_next(input)?; }
                 }
            }
            ModelPattern::Parenthesized(inner, _span) => self.generate_delimited_step(inner, "(", ")"),
            ModelPattern::Bracketed(inner, _span) => self.generate_delimited_step(inner, "[", "]"),
            ModelPattern::Braced(inner, _span) => self.generate_delimited_step(inner, "{", "}"),
            ModelPattern::Cut(_) => quote_spanned! {span=> },
            ModelPattern::Recover { .. } => quote_spanned! {span=>
                compile_error!("Recover not yet supported in winnow-grammar");
            }
        }
    }

    fn generate_delimited_step(&self, inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
        let span = Span::mixed_site();
        let steps: Vec<TokenStream> = inner.iter().map(|p| self.generate_step(p)).collect();
        quote_spanned! {span=>
            let _ = (ws, literal(#open)).parse_next(input)?;
            #(#steps)*
            let _ = (ws, literal(#close)).parse_next(input)?;
        }
    }

    fn generate_rule_call_parser(&self, rule_name: &syn::Ident, args: &[syn::Lit]) -> TokenStream {
        let span = Span::mixed_site();
        let name_str = rule_name.to_string();
        
        // If it's a user-defined rule, always use the user's rule.
        if self.user_rules.contains(&name_str) {
            let fn_name = format_ident!("parse_{}", rule_name, span = span);
            if args.is_empty() {
                return quote_spanned! {span=> #fn_name };
            } else {
                return quote_spanned! {span=> |i: &mut _| #fn_name(i, #(#args),*) };
            }
        }

        // Otherwise, check for built-ins
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
                // Unknown rule or external (unprefixed) function.
                if args.is_empty() {
                    quote_spanned! {span=> #rule_name }
                } else {
                    quote_spanned! {span=> |i: &mut _| #rule_name(i, #(#args),*) }
                }
            }
        }
    }

    fn generate_parser_expr(&self, pattern: &ModelPattern) -> TokenStream {
        let span = Span::mixed_site();
        match pattern {
            ModelPattern::SpanBinding(inner, _, _) => {
                let p = self.generate_parser_expr(inner);
                quote_spanned! {span=> #p.with_span().map(|(v, _)| v) }
            }
            ModelPattern::RuleCall { rule_name, args, .. } => {
                self.generate_rule_call_parser(rule_name, args)
            }
            ModelPattern::Lit(lit_str) => {
                let s = lit_str.value();
                quote_spanned! {span=>
                    (ws, literal(#s)).map(|(_, s)| s)
                }
            }
            ModelPattern::Group(alternatives, _) => {
                let alts: Vec<TokenStream> = alternatives.iter().map(|seq: &Vec<ModelPattern>| {
                    let seq_parsers: Vec<TokenStream> = seq.iter().map(|p| self.generate_parser_expr(p)).collect();
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
                let p = self.generate_parser_expr(inner);
                quote_spanned! {span=> opt(#p) }
            }
            ModelPattern::Repeat(inner, _span) => {
                let p = self.generate_parser_expr(inner);
                quote_spanned! {span=> repeat(0.., #p) }
            }
            ModelPattern::Plus(inner, _span) => {
                let p = self.generate_parser_expr(inner);
                quote_spanned! {span=> repeat(1.., #p) }
            }
            ModelPattern::Parenthesized(inner, _) => self.generate_delimited_expr(inner, "(", ")"),
            ModelPattern::Bracketed(inner, _) => self.generate_delimited_expr(inner, "[", "]"),
            ModelPattern::Braced(inner, _) => self.generate_delimited_expr(inner, "{", "}"),
            ModelPattern::Cut(_) => quote_spanned! {span=> ::winnow::combinator::empty },
            _ => quote_spanned! {span=>
                compile_error!("Unsupported pattern type in generate_parser_expr")
            }
        }
    }

    fn generate_delimited_expr(&self, inner: &[ModelPattern], open: &str, close: &str) -> TokenStream {
        let span = Span::mixed_site();
        let seq_parsers: Vec<TokenStream> = inner.iter().map(|p| self.generate_parser_expr(p)).collect();
        let inner_parser = if seq_parsers.len() == 1 {
            quote_spanned! {span=> #(#seq_parsers)* }
        } else {
            quote_spanned! {span=> ( #(#seq_parsers),* ) }
        };
        
        quote_spanned! {span=>
            delimited((ws, literal(#open)), #inner_parser, (ws, literal(#close)))
        }
    }
}

// Helper (outside struct since it doesn't need context)
fn get_inner_binding(pattern: &ModelPattern) -> Option<&syn::Ident> {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => binding.as_ref(),
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
        ModelPattern::SpanBinding(inner, _, _) => get_inner_binding(inner),
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
