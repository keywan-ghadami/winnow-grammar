use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use std::collections::HashSet;
use syn_grammar_model::{
    analysis,
    model::{GrammarDefinition, ModelPattern, Rule, RuleVariant},
};

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
        let user_rules = grammar.rules.iter().map(|r| r.name.to_string()).collect();
        Self {
            grammar,
            user_rules,
        }
    }

    fn generate(&mut self) -> syn::Result<TokenStream> {
        let grammar_name = &self.grammar.name;
        let span = Span::mixed_site();
        let use_statements = &self.grammar.uses;

        let has_user_ws = self.user_rules.contains("ws");

        let rules = self.grammar.rules.iter().map(|r| self.generate_rule(r));

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
                    I: ::winnow::stream::Stream<Token = char> + ::winnow::stream::StreamIsPartial + for<'a> ::winnow::stream::Compare<&'a str>,
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

        let params: Vec<TokenStream> = rule
            .params
            .iter()
            .map(|(name, ty)| {
                quote! { #name: #ty }
            })
            .collect();

        let (recursive_refs, base_refs) =
            analysis::split_left_recursive(&rule.name, &rule.variants);

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
            let loop_body =
                self.generate_recursive_loop_body(&recursive_owned, ret_type, &lhs_ident);

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
                I: ::winnow::stream::Stream<Token = char>
                   + ::winnow::stream::StreamIsPartial
                   + ::winnow::stream::Location
                   + ::winnow::stream::Compare<char>
                   + for<'a> ::winnow::stream::Compare<&'a str>,
                <I as ::winnow::stream::Stream>::Slice: ::winnow::stream::AsBStr + AsRef<str> + std::fmt::Display,
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

    fn generate_variants_body(
        &self,
        variants: &[RuleVariant],
        ret_type: &syn::Type,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let variant_parsers = variants.iter().map(|v| {
            let steps = self.generate_sequence_steps(&v.pattern, false);
            let action = &v.action;
            quote_spanned! {span=>
                |input: &mut I| -> ModalResult<#ret_type> {
                    #steps
                    Ok(#action)
                }
            }
        });

        if variants.len() == 1 {
            let v = &variants[0];
            let steps = self.generate_sequence_steps(&v.pattern, false);
            let action = &v.action;
            quote_spanned! {span=>
                {
                    #steps
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

    fn generate_recursive_loop_body(
        &self,
        variants: &[RuleVariant],
        ret_type: &syn::Type,
        lhs_ident: &syn::Ident,
    ) -> TokenStream {
        let span = Span::mixed_site();

        let arms = variants.iter().map(|v| {
            let lhs_binding = match &v.pattern[0] {
                ModelPattern::RuleCall {
                    binding: Some(b), ..
                } => Some(b),
                _ => None,
            };

            let bind_lhs = if let Some(b) = lhs_binding {
                quote! { let #b = #lhs_ident.clone(); }
            } else {
                quote! {}
            };

            // Recursion always consumes the first pattern (the recursive call).
            // We check if that first pattern was 'cut'. If so, the rest is cut.
            // But ModelPattern::RuleCall cannot be a Cut itself.
            // However, if the pattern list is `[Recurse, Cut, ...]`, then `patterns` below starts with `Cut`.

            let patterns = &v.pattern[1..];
            let steps = self.generate_sequence_steps(patterns, false);
            let action = &v.action;

            quote_spanned! {span=>
                {
                    let checkpoint = ::winnow::stream::Stream::checkpoint(input);
                    let attempt = (|| -> ModalResult<#ret_type> {
                        #steps
                        #bind_lhs
                        Ok(#action)
                    })();

                    match attempt {
                        Ok(val) => {
                            #lhs_ident = val;
                            continue;
                        },
                        Err(e) => {
                            match e {
                                // If it's a backtrack error, we reset and try next variant.
                                ::winnow::error::ErrMode::Backtrack(_) => {
                                    ::winnow::stream::Stream::reset(input, &checkpoint);
                                }
                                // If it's Cut or Incomplete, we propagate.
                                _ => return Err(e),
                            }
                        }
                    }
                }
            }
        });

        quote_spanned! {span=>
            #(#arms)*
        }
    }

    fn generate_sequence_steps(&self, patterns: &[ModelPattern], mut in_cut: bool) -> TokenStream {
        let mut steps = Vec::new();
        for p in patterns {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
                continue;
            }
            steps.push(self.generate_step(p, in_cut));
        }
        quote! { #(#steps)* }
    }

    fn generate_step(&self, pattern: &ModelPattern, in_cut: bool) -> TokenStream {
        let span = Span::mixed_site();

        // Special case: Unwrap groups to allow bindings to escape to the current scope.
        // But only if they are simple sequences. If they are alts, generate_parser_expr handles them (returning a value).
        if let ModelPattern::Group(alts, _) = pattern {
            if alts.len() == 1 {
                return self.generate_sequence_steps(&alts[0], in_cut);
            }
        }

        // Special case: Parenthesized/Bracketed/Braced need to emit statements (open, inner, close)
        // to preserve bindings from inner.
        match pattern {
            ModelPattern::Parenthesized(inner, _) => {
                return self.generate_delimited_step(inner, "(", ")", in_cut)
            }
            ModelPattern::Bracketed(inner, _) => {
                return self.generate_delimited_step(inner, "[", "]", in_cut)
            }
            ModelPattern::Braced(inner, _) => {
                return self.generate_delimited_step(inner, "{", "}", in_cut)
            }
            ModelPattern::Recover { .. } => {
                return quote_spanned! {span=>
                    compile_error!("Recover not yet supported in winnow-grammar");
                };
            }
            _ => {}
        }

        // Default: Generate a parser expression and run it.
        let parser_expr = self.generate_parser_expr(pattern);

        // If we are in cut mode, wrap the parser.
        let parser_expr = if in_cut {
            quote_spanned! {span=> ::winnow::combinator::cut_err(#parser_expr) }
        } else {
            parser_expr
        };

        // Bind result if needed
        let binding = get_inner_binding(pattern);
        match binding {
            Some(name) => match pattern {
                ModelPattern::SpanBinding(_, span_var, _) => quote_spanned! {span=>
                    let (#name, #span_var) = #parser_expr.with_span().parse_next(input)?;
                },
                ModelPattern::Repeat(_, _) | ModelPattern::Plus(_, _) => quote_spanned! {span=>
                    let #name: Vec<_> = #parser_expr.parse_next(input)?;
                },
                _ => quote_spanned! {span=>
                    let #name = #parser_expr.parse_next(input)?;
                },
            },
            None => match pattern {
                ModelPattern::SpanBinding(_, span_var, _) => quote_spanned! {span=>
                    let (_, #span_var) = #parser_expr.with_span().parse_next(input)?;
                },
                ModelPattern::Repeat(_, _) | ModelPattern::Plus(_, _) => quote_spanned! {span=>
                    let _: Vec<_> = #parser_expr.parse_next(input)?;
                },
                _ => quote_spanned! {span=>
                    let _ = #parser_expr.parse_next(input)?;
                },
            },
        }
    }

    fn generate_delimited_step(
        &self,
        inner: &[ModelPattern],
        open: &str,
        close: &str,
        in_cut: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();

        // Open delimiter
        let open_parser = quote_spanned! {span=> (ws, literal(#open)) };
        let open_stmt = if in_cut {
            quote_spanned! {span=> let _ = ::winnow::combinator::cut_err(#open_parser).parse_next(input)?; }
        } else {
            quote_spanned! {span=> let _ = #open_parser.parse_next(input)?; }
        };

        // Inner steps
        // Note: inner sequence might trigger cut mode itself!
        // We need to know if the inner sequence ends in cut mode to decide for the closing delimiter.
        let inner_steps = self.generate_sequence_steps(inner, in_cut);

        // Check if inner triggers cut
        let inner_triggers_cut = inner.iter().any(|p| matches!(p, ModelPattern::Cut(_)));
        let final_cut = in_cut || inner_triggers_cut;

        // Close delimiter
        let close_parser = quote_spanned! {span=> (ws, literal(#close)) };
        let close_stmt = if final_cut {
            quote_spanned! {span=> let _ = ::winnow::combinator::cut_err(#close_parser).parse_next(input)?; }
        } else {
            quote_spanned! {span=> let _ = #close_parser.parse_next(input)?; }
        };

        quote_spanned! {span=>
            #open_stmt
            #inner_steps
            #close_stmt
        }
    }

    fn generate_rule_call_parser(&self, rule_name: &syn::Ident, args: &[syn::Lit]) -> TokenStream {
        let span = Span::mixed_site();
        let name_str = rule_name.to_string();

        if self.user_rules.contains(&name_str) {
            let fn_name = format_ident!("parse_{}", rule_name, span = span);
            if args.is_empty() {
                return quote_spanned! {span=> #fn_name };
            } else {
                return quote_spanned! {span=> (|i: &mut _| #fn_name(i, #(#args),*)) };
            }
        }

        match name_str.as_str() {
            "ident" => quote_spanned! {span=>
                (ws, ::winnow::token::take_while(1.., |c| ::winnow::stream::AsChar::as_char(c).is_alphanumeric() || ::winnow::stream::AsChar::as_char(c) == '_'))
                    .map(|(_, s)| AsRef::<str>::as_ref(&s).to_string())
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
                .map(|(_, s)| AsRef::<str>::as_ref(&s).to_string())
            },
            "char" => quote_spanned! {span=>
                (ws, delimited(
                    '\'',
                    alt((
                        ::winnow::combinator::preceded('\\', ::winnow::token::any).map(|c| {
                             match c {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '\\' => '\\',
                                '\'' => '\'',
                                '"' => '"',
                                '0' => '\0',
                                _ => c // fallback
                             }
                        }),
                        ::winnow::token::none_of(['\''])
                    )),
                    '\''
                ))
                .map(|(_, c)| c)
            },
            _ => {
                if args.is_empty() {
                    quote_spanned! {span=> #rule_name }
                } else {
                    quote_spanned! {span=> (|i: &mut _| #rule_name(i, #(#args),*)) }
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
            ModelPattern::RuleCall {
                rule_name, args, ..
            } => self.generate_rule_call_parser(rule_name, args),
            ModelPattern::Lit(lit_str) => {
                let s = lit_str.value();
                quote_spanned! {span=>
                    (ws, literal(#s)).map(|(_, s)| s)
                }
            }
            ModelPattern::Group(alternatives, _) => {
                let alts: Vec<TokenStream> = alternatives
                    .iter()
                    .map(|seq| self.generate_sequence_parser(seq))
                    .collect();
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
            ModelPattern::Cut(_) => quote_spanned! {span=> ::winnow::combinator::empty }, // Should be handled by sequence logic, but fallback to empty
            ModelPattern::Recover { .. } => quote_spanned! {span=>
                compile_error!("Recover not yet supported in winnow-grammar");
            },
        }
    }

    fn generate_sequence_parser(&self, seq: &[ModelPattern]) -> TokenStream {
        let span = Span::mixed_site();
        let mut parsers = Vec::new();
        let mut in_cut = false;

        for p in seq {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
                continue;
            }

            let p_expr = self.generate_parser_expr(p);
            if in_cut {
                parsers.push(quote_spanned! {span=> ::winnow::combinator::cut_err(#p_expr) });
            } else {
                parsers.push(p_expr);
            }
        }

        if parsers.len() == 1 {
            quote_spanned! {span=> #(#parsers)* }
        } else {
            quote_spanned! {span=> ( #(#parsers),* ) }
        }
    }

    fn generate_delimited_expr(
        &self,
        inner: &[ModelPattern],
        open: &str,
        close: &str,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let inner_parser = self.generate_sequence_parser(inner);

        quote_spanned! {span=>
            delimited((ws, literal(#open)), #inner_parser, (ws, literal(#close)))
        }
    }
}

fn get_inner_binding(pattern: &ModelPattern) -> Option<&syn::Ident> {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => binding.as_ref(),
        ModelPattern::Group(alts, _) => {
            if alts.len() == 1 && alts[0].len() == 1 {
                get_inner_binding(&alts[0][0])
            } else {
                None
            }
        }
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
        }
        _ => None,
    }
}
