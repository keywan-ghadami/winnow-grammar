use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use std::collections::HashSet;
use syn_grammar_model::{
    analysis,
    model::{Argument, GrammarDefinition, ModelPattern, Rule, RuleVariant}
};

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    let mut codegen = Codegen::new(&grammar);
    codegen.generate()
}

struct Codegen<'a> {
    grammar: &'a GrammarDefinition,
    user_rules: HashSet<String>,
    input_ident: syn::Ident,
}

impl<'a> Codegen<'a> {
    fn new(grammar: &'a GrammarDefinition) -> Self {
        let user_rules = grammar.rules.iter().map(|r| r.name.to_string()).collect();
        Self {
            grammar,
            user_rules,
            input_ident: format_ident!("input", span = Span::call_site()),
        }
    }

    fn generate(&mut self) -> syn::Result<TokenStream> {
        let grammar_name = &self.grammar.name;
        let span = Span::mixed_site();
        let use_statements = &self.grammar.uses;
        let input = &self.input_ident;

        let has_user_ws = self.user_rules.contains("WS");

        let rules = self.grammar.rules.iter().map(|r| self.generate_rule(r));

        let use_super = quote_spanned! {Span::call_site()=> use super::*; };

        // If user defined WS, we alias WS to parse_WS_inner so that internal usage (and wrappers) call the inner parser directly.
        let ws_parser = if has_user_ws {
            quote_spanned! {span=>
                #[allow(unused_imports)]
                use parse_WS_inner as WS;
            }
        } else {
            quote_spanned! {span=>
                // Whitespace handling (similar to syn)
                #[allow(dead_code)]
                fn WS<'a, I>(#input: &mut I) -> ::winnow::Result<()>
                where
                    I: ::winnow::stream::Stream<Token = char, Slice = &'a str> + ::winnow::stream::StreamIsPartial + for<'b> ::winnow::stream::Compare<&'b str>,
                {
                    ::winnow::ascii::multispace0.parse_next(#input).map(|_| ())
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
                use ::winnow::combinator::{alt, repeat, opt, delimited, preceded};

                #ws_parser

                #(#rules)*
            }
        })
    }

    fn generate_rule(&self, rule: &Rule) -> TokenStream {
        let rule_name = &rule.name;
        let rule_name_str = rule_name.to_string();
        let is_ws_rule = rule_name_str == "WS";
        let span = Span::mixed_site();
        let fn_name = format_ident!("parse_{}", rule_name, span = span);
        let inner_fn_name = format_ident!("parse_{}_inner", rule_name, span = span);
        let ret_type = &rule.return_type;
        let input = &self.input_ident;

        let mut extra_generics = Vec::new();
        let mut params_tokens = Vec::new();
        let mut arg_names = Vec::new();

        for param in &rule.params {
            let name = &param.name;
            let ty = &param.ty;
            arg_names.push(name.clone());
            match ty {
                Some(t) => params_tokens.push(quote! { mut #name: #t }),
                None => {
                    let output_type = format_ident!("Output_{}", name, span = Span::mixed_site());
                    extra_generics.push(output_type.clone());
                    params_tokens.push(quote! {
                        mut #name: impl ::winnow::Parser<I, #output_type, ::winnow::error::ContextError>
                    });
                }
            }
        }

        let (recursive_refs, base_refs) =
            analysis::split_left_recursive(&rule.name, &rule.variants);

        let lhs_ident = format_ident!("lhs", span = span);
        let is_lexical = rule.is_lexical || is_ws_rule;

        let body = if recursive_refs.is_empty() {
            self.generate_variants_body(&rule.variants, ret_type, is_lexical, true)
        // is_rule_start=true
        } else if base_refs.is_empty() {
            quote_spanned! {span=>
                compile_error!("Left-recursive rule requires at least one non-recursive base variant.")
            }
        } else {
            let base_owned: Vec<RuleVariant> = base_refs.into_iter().cloned().collect();
            let recursive_owned: Vec<RuleVariant> = recursive_refs.into_iter().cloned().collect();

            let base_parser = self.generate_variants_body(&base_owned, ret_type, is_lexical, true);
            let loop_body = self.generate_recursive_loop_body(
                &recursive_owned,
                ret_type,
                &lhs_ident,
                is_lexical,
            );

            quote_spanned! {span=>
                let mut #lhs_ident = #base_parser?;
                loop {
                    #loop_body
                    break;
                }
                Ok(#lhs_ident)
            }
        };

        let vis = if rule.is_pub {
            quote! { pub }
        } else {
            quote! {}
        };

        let gen_params = &rule.generics.params;
        let gen_where = &rule.generics.where_clause;

        let mut all_generics = quote! { 'a };
        if !gen_params.is_empty() {
            all_generics.extend(quote! {, #gen_params});
        }
        all_generics.extend(quote! {, I});
        if !extra_generics.is_empty() {
            all_generics.extend(quote! {, #(#extra_generics),*});
        }

        let where_preds = if let Some(w) = gen_where {
            let p = &w.predicates;
            quote! { #p, }
        } else {
            quote! {}
        };

        let ws_shadow = if is_ws_rule {
            quote_spanned! {span=>
                #[allow(dead_code)]
                fn WS<I>(_: &mut I) -> ::winnow::Result<()>
                where
                    I: ::winnow::stream::Stream,
                {
                    Ok(())
                }
            }
        } else {
            quote! {}
        };

        let inner_fn = quote_spanned! {span=>
            #[allow(dead_code)]
            fn #inner_fn_name<#all_generics>(#input: &mut I, #(#params_tokens),*) -> ::winnow::Result<#ret_type>
            where
                #where_preds
                I: ::winnow::stream::Stream<Token = char, Slice = &'a str>
                   + ::winnow::stream::StreamIsPartial
                   + ::winnow::stream::Location
                   + ::winnow::stream::Compare<char>
                   + for<'b> ::winnow::stream::Compare<&'b str>
                   + ::winnow::stream::Compare<::winnow::ascii::Caseless<&'static str>>
                   + ::winnow::stream::AsBStr
                   + ::winnow::stream::FindSlice<char>
                   + ::winnow::stream::FindSlice<&'static str>,
                <I as ::winnow::stream::Stream>::IterOffsets: Clone,
            {
                use ::winnow::Parser;
                use ::winnow::error::ContextError;

                #ws_shadow

                let mut parser = (|#input: &mut I| -> ::winnow::Result<#ret_type> {
                    #body
                })
                .context(::winnow::error::StrContext::Label(#rule_name_str));

                #[cfg(feature = "trace")]
                {
                    ::winnow::combinator::trace(#rule_name_str, parser).parse_next(#input)
                }

                #[cfg(not(feature = "trace"))]
                {
                    parser.parse_next(#input)
                }
            }
        };

        let mut outer_generics = quote!{};
        if !gen_params.is_empty() {
            outer_generics.extend(quote!{<#gen_params>});
        }
        let err_type = quote_spanned! { span=> ::winnow::error::ContextError };

        let outer_fn_body = quote! {
            move |input: &mut _| -> ::winnow::Result<#ret_type> {
                // Public API wrapper to handle whitespace and EOF
                let _ = WS(input)?;

                // Call inner rule
                let result = #inner_fn_name(input, #(#arg_names),*)?;

                let _ = WS(input)?;

                // EOF check
                ::winnow::combinator::eof.parse_next(input)?;

                Ok(result)
            }
        };

        let outer_fn = match rule.return_type_kind {
            analysis::ReturnTypeKind::Borrowed => {
                let mut outer_generics_with_lifetime = quote!{<'a>};
                if !gen_params.is_empty() {
                    outer_generics_with_lifetime.extend(quote!{, #gen_params});
                }

                quote_spanned! {span=>
                    #vis fn #fn_name #outer_generics_with_lifetime (#(#params_tokens),*) -> impl ::winnow::Parser<
                        ::winnow::stream::LocatingSlice<&'a str>,
                        #ret_type,
                        #err_type
                    >
                    where
                        #gen_where
                    {
                        #outer_fn_body
                    }
                }
            },
            analysis::ReturnTypeKind::Primitive => {
                quote_spanned! {span=>
                    #vis fn #fn_name #outer_generics (#(#params_tokens),*) -> impl for<'a> ::winnow::Parser<
                        ::winnow::stream::LocatingSlice<&'a str>,
                        #ret_type,
                        #err_type
                    >
                    where
                        #gen_where
                    {
                        #outer_fn_body
                    }
                }
            },
            analysis::ReturnTypeKind::Empty => {
                quote_spanned! {span=>
                    #vis fn #fn_name #outer_generics (#(#params_tokens),*) -> impl for<'a> ::winnow::Parser<
                        ::winnow::stream::LocatingSlice<&'a str>,
                        #ret_type,
                        #err_type
                    >
                    where
                        #gen_where
                    {
                        #outer_fn_body
                    }
                }
            }
        };


        quote! {
            #inner_fn
            #outer_fn
        }
    }

    fn generate_variants_body(
        &self,
        variants: &[RuleVariant],
        _ret_type: &syn::Type,
        is_lexical: bool,
        is_rule_start: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;

        let variant_parsers = variants.iter().map(|v| {
            let mut steps_code = TokenStream::new();
            let use_with_span = v.with_span;
            let is_explicit = v.is_explicit;

            // 1. Optional Leading WS
            if is_rule_start && !is_lexical {
                steps_code.extend(quote! { let _ = WS.parse_next(#input)?; });
            }

            // 2. Capture Start
            if use_with_span {
                steps_code
                    .extend(quote! { let start = ::winnow::stream::Location::location(#input); });
            }

            // 3. Parse Steps
            let sequence_steps = self.generate_sequence_steps(&v.pattern, false, is_lexical);
            steps_code.extend(sequence_steps);

            // 4. Capture End and Define _span
            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::stream::Location::location(#input); });
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            let action_str = action.to_string();
            let state_injection = if action_str.contains("_state") {
                quote! { let _state = ::winnow::stream::Stateful::state_mut(#input); }
            } else {
                quote! {}
            };

            let final_expr = if use_with_span && !is_explicit {
                // Implicit action -> use WithSpan
                quote! {
                    Ok(::grammar_kit::WithSpan::with_span({ #action }, _span))
                }
            } else {
                // Explicit action (user handles _span if needed) OR no span requested
                quote! {
                    Ok({ #action })
                }
            };

            quote_spanned! {span=>
                |#input: &mut I| -> ::winnow::Result<_> {
                    #steps_code
                    #state_injection
                    #final_expr
                }
            }
        });

        if variants.len() == 1 {
            let v = &variants[0];
            let mut steps_code = TokenStream::new();
            let use_with_span = v.with_span;
            let is_explicit = v.is_explicit;

            if is_rule_start && !is_lexical {
                steps_code.extend(quote! { let _ = WS.parse_next(#input)?; });
            }

            if use_with_span {
                steps_code
                    .extend(quote! { let start = ::winnow::stream::Location::location(#input); });
            }

            steps_code.extend(self.generate_sequence_steps(&v.pattern, false, is_lexical));

            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::stream::Location::location(#input); });
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            let action_str = action.to_string();
            let state_injection = if action_str.contains("_state") {
                quote! { let _state = ::winnow::stream::Stateful::state_mut(#input); }
            } else {
                quote! {}
            };

            let final_expr = if use_with_span && !is_explicit {
                quote! {
                    Ok(::grammar_kit::WithSpan::with_span({ #action }, _span))
                }
            } else {
                quote! {
                    Ok(#action)
                }
            };

            quote_spanned! {span=>
                {
                    #steps_code
                    #state_injection
                    #final_expr
                }
            }
        } else {
            quote_spanned! {span=>
                alt((
                    #(#variant_parsers),*
                )).parse_next(#input)
            }
        }
    }

    fn generate_recursive_loop_body(
        &self,
        variants: &[RuleVariant],
        ret_type: &syn::Type,
        lhs_ident: &syn::Ident,
        is_lexical: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;

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

            let patterns = &v.pattern[1..]; // pattern[0] is LHS

            let mut steps_code = TokenStream::new();
            let use_with_span = v.with_span;
            let is_explicit = v.is_explicit;

            // In recursive step, LHS is already parsed.
            // pattern[1] follows LHS.
            // If !is_lexical, we must consume ws between LHS and pattern[1].
            if !is_lexical {
                steps_code.extend(quote! { let _ = WS.parse_next(#input)?; });
            }

            steps_code.extend(self.generate_sequence_steps(patterns, false, is_lexical));

            // Capture end and define _span
            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::stream::Location::location(#input); });
                // For recursive steps, _span refers to the suffix extension.
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            let action_str = action.to_string();
            let state_injection = if action_str.contains("_state") {
                quote! { let _state = ::winnow::stream::Stateful::state_mut(#input); }
            } else {
                quote! {}
            };

            let final_expr = if use_with_span && !is_explicit {
                // If implicit action, use WithSpan.
                // NOTE: Here we probably want the span of LHS + Suffix.
                // But _span is just Suffix.
                // We rely on lhs_ident.span (if it exists) to get full span.
                // This assumes implicit action types implement .span().

                quote! {
                     // Try to construct full span if possible, else use suffix span?
                     // For implicit actions, we assume result type is same as LHS type.
                     // And if it was created via WithSpan, it should have a span.
                     // But strictly speaking, WithSpan trait injects span.
                     // We should pass full span.

                     let full_span = #lhs_ident.span.start .. end;
                     Ok(::grammar_kit::WithSpan::with_span({ #action }, full_span))
                }
            } else {
                quote! {
                    Ok(#action)
                }
            };

            // Start capture needs to happen before steps_code
            // 'start' here refers to start of suffix
            let start_capture = if use_with_span {
                quote! { let start = ::winnow::stream::Location::location(#input); }
            } else {
                quote! {}
            };

            quote_spanned! {span=>
                {
                    let checkpoint = ::winnow::stream::Stream::checkpoint(#input);
                    #start_capture
                    let attempt = (|| -> ::winnow::Result<#ret_type> {
                        #steps_code
                        #bind_lhs
                        #state_injection
                        #final_expr
                    })();

                    match attempt {
                        Ok(val) => {
                            #lhs_ident = val;
                            continue;
                        },
                        Err(e) => {
                            if e.is_fatal() {
                                return Err(e);
                            } else {
                                ::winnow::stream::Stream::reset(#input, &checkpoint);
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

    // ... rest of the file ...
    fn generate_sequence_steps(
        &self,
        patterns: &[ModelPattern],
        mut in_cut: bool,
        is_lexical: bool,
    ) -> TokenStream {
        let mut steps = Vec::new();
        let input = &self.input_ident;

        for (i, p) in patterns.iter().enumerate() {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
            }

            if i > 0 && !is_lexical {
                steps.push(quote! { let _ = WS.parse_next(#input)?; });
            }

            steps.push(self.generate_step(p, in_cut, is_lexical));
        }
        quote! { #(#steps)* }
    }

    fn generate_step(&self, pattern: &ModelPattern, in_cut: bool, is_lexical: bool) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;

        if let ModelPattern::Group { alts, .. } = pattern {
            if alts.len() == 1 {
                return self.generate_sequence_steps(&alts[0].0, in_cut, is_lexical);
            }
        }

        match pattern {
            ModelPattern::Parenthesized(inner, _) => {
                return self.generate_delimited_step(inner, "(", ")", in_cut, is_lexical)
            }
            ModelPattern::Bracketed(inner, _) => {
                return self.generate_delimited_step(inner, "[", "]", in_cut, is_lexical)
            }
            ModelPattern::Braced(inner, _) => {
                return self.generate_delimited_step(inner, "{", "}", in_cut, is_lexical)
            }
            _ => {}
        }

        let binding = get_inner_binding(pattern);
        let is_discarded = binding.is_none();

        let parser_expr = self.generate_parser_expr(pattern, is_lexical, is_discarded);
        let parser_expr = if in_cut {
            quote_spanned! {span=> ::winnow::combinator::cut_err(#parser_expr) }
        } else {
            parser_expr
        };

        match binding {
            Some(name) => match pattern {
                ModelPattern::SpanBinding(_, span_var, _) => quote_spanned! {span=>
                    let (#name, #span_var) = #parser_expr.with_span().parse_next(#input)?;
                },
                ModelPattern::Repeat(_, _)
                | ModelPattern::Plus(_, _)
                | ModelPattern::Count { .. } => quote_spanned! {span=>
                    let #name: Vec<_> = #parser_expr.parse_next(#input)?;
                },
                _ => quote_spanned! {span=>
                    let #name = #parser_expr.parse_next(#input)?;
                },
            },
            None => match pattern {
                ModelPattern::SpanBinding(_, span_var, _) => quote_spanned! {span=>
                    let (_, #span_var) = #parser_expr.with_span().parse_next(#input)?;
                },
                // Explicitly discard result for unbinded repetitions to help type inference (e.g. Accumulate<()>)
                ModelPattern::Repeat(_, _) | ModelPattern::Plus(_, _) => quote_spanned! {span=>
                    let _: () = #parser_expr.parse_next(#input)?;
                },
                _ => quote_spanned! {span=>
                    let _ = #parser_expr.parse_next(#input)?;
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
        is_lexical: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;

        // Sequence: Open, (ws), Inner, (ws), Close

        let open_parser = quote_spanned! {span=> literal(#open) };
        let open_stmt = if in_cut {
            quote_spanned! {span=> let _ = ::winnow::combinator::cut_err(#open_parser).parse_next(#input)?; }
        } else {
            quote_spanned! {span=> let _ = #open_parser.parse_next(#input)?; }
        };

        // Infix WS between Open and Inner
        let ws_before_inner = if !is_lexical {
            quote_spanned! {span=> let _ = WS.parse_next(#input)?; }
        } else {
            quote! {}
        };

        let inner_steps = self.generate_sequence_steps(inner, in_cut, is_lexical);

        let inner_triggers_cut = inner.iter().any(|p| matches!(p, ModelPattern::Cut(_)));
        let final_cut = in_cut || inner_triggers_cut;

        // Infix WS between Inner and Close
        let ws_before_close = if !is_lexical {
            quote_spanned! {span=> let _ = WS.parse_next(#input)?; }
        } else {
            quote! {}
        };

        let close_parser = quote_spanned! {span=> literal(#close) };
        let close_stmt = if final_cut {
            quote_spanned! {span=> let _ = ::winnow::combinator::cut_err(#close_parser).parse_next(#input)?; }
        } else {
            quote_spanned! {span=> let _ = #close_parser.parse_next(#input)?; }
        };

        quote_spanned! {span=>
            #open_stmt
            #ws_before_inner
            #inner_steps
            #ws_before_close
            #close_stmt
        }
    }

    fn generate_argument_expr(&self, arg: &Argument, is_lexical: bool) -> TokenStream {
        let span = Span::mixed_site();
        let pattern = match arg {
            Argument::Positional(p) => p,
            Argument::Named(_, p) => p,
        };

        match pattern {
            ModelPattern::Lit { lit, .. } => match lit {
                syn::Lit::Int(_) | syn::Lit::Bool(_) => quote_spanned! {span=> #lit },
                _ => self.generate_parser_expr(pattern, is_lexical, false),
            },
            _ => self.generate_parser_expr(pattern, is_lexical, false),
        }
    }

    fn generate_rule_call_parser(
        &self,
        rule_path: &syn::Path,
        args: &[Argument],
        is_lexical: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let rule_name = &rule_path.segments.last().unwrap().ident;
        let name_str = rule_name.to_string();

        if self.user_rules.contains(&name_str) {
            let fn_name = format_ident!("parse_{}_inner", rule_name, span = span);
            if args.is_empty() {
                return quote_spanned! {span=> (move |i: &mut _| #fn_name(i)) };
            } else {
                let arg_exprs = args
                    .iter()
                    .map(|arg| self.generate_argument_expr(arg, is_lexical));
                return quote_spanned! {span=> (move |i: &mut _| #fn_name(i, #(#arg_exprs),*)) };
            }
        }

        match name_str.as_str() {
            "ident" => quote_spanned! {span=>
                ::winnow::token::take_while(1.., |c| ::winnow::stream::AsChar::as_char(c).is_alphanumeric() || ::winnow::stream::AsChar::as_char(c) == '_')
            },
            "string" => quote_spanned! {span=>
                 delimited(
                    '"',
                    ::winnow::ascii::take_escaped(
                        ::winnow::token::none_of(['\\', '"']),
                        '\\',
                        ::winnow::token::one_of(['\\', '"'])
                    ),
                    '"'
                )
            },
            "char" => quote_spanned! {span=>
                delimited(
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
                        ::winnow::token::none_of(['\\', '\''])
                    )),
                    '\''
                )
            },
            "any" => quote_spanned! {span=> ::winnow::token::any },
            "alpha1" => {
                quote_spanned! {span=> ::winnow::ascii::alpha1 }
            }
            "digit1" => {
                quote_spanned! {span=> ::winnow::ascii::digit1 }
            }
            "hex_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit0 }
            }
            "hex_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit1 }
            }
            "oct_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit0 }
            }
            "oct_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit1 }
            }
            "binary_digit0" => quote_spanned! {span=>
                ::winnow::token::take_while(0.., |c| c == '0' || c == '1')
            },
            "binary_digit1" => quote_spanned! {span=>
                ::winnow::token::take_while(1.., |c| c == '0' || c == '1')
            },
            "space0" => {
                quote_spanned! {span=> ::winnow::ascii::space0 }
            }
            "space1" => {
                quote_spanned! {span=> ::winnow::ascii::space1 }
            }
            "multispace0" => {
                quote_spanned! {span=> ::winnow::ascii::multispace0 }
            }
            "multispace1" => {
                quote_spanned! {span=> ::winnow::ascii::multispace1 }
            }
            "line_ending" => {
                quote_spanned! {span=> ::winnow::ascii::line_ending }
            }
            "empty" => quote_spanned! {span=> ::winnow::combinator::empty },
            "eof" => quote_spanned! {span=> ::winnow::combinator::eof },

            "u8" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, u8, _> },
            "u16" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, u16, _> },
            "u32" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, u32, _> },
            "u64" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, u64, _> },
            "u128" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, u128, _> },
            "usize" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<_, usize, _> },
            "i8" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, i8, _> },
            "i16" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, i16, _> },
            "i32" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, i32, _> },
            "i64" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, i64, _> },
            "i128" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, i128, _> },
            "isize" => quote_spanned! {span=> ::winnow::ascii::dec_int::<_, isize, _> },
            "f32" => quote_spanned! {span=> ::winnow::ascii::float::<_, f32, _> },
            "f64" => quote_spanned! {span=> ::winnow::ascii::float::<_, f64, _> },
            "bool" => quote_spanned! {span=>
                ::winnow::combinator::alt((
                    ::winnow::token::literal("true").map(|_| true),
                    ::winnow::token::literal("false").map(|_| false),
                ))
            },
            _ => {
                if args.is_empty() {
                    quote_spanned! {span=> (move |i: &mut _| ::winnow::Parser::parse_next(&mut #rule_path, i)) }
                } else {
                    let arg_exprs = args
                        .iter()
                        .map(|arg| self.generate_argument_expr(arg, is_lexical));
                    quote_spanned! {span=> (move |i: &mut _| #rule_path(i, #(#arg_exprs),*)) }
                }
            }
        }
    }

    fn generate_parser_expr(
        &self,
        pattern: &ModelPattern,
        is_lexical: bool,
        is_discarded: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        match pattern {
            ModelPattern::SpanBinding(inner, _, _) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                quote_spanned! {span=> #p.with_span().map(|(v, _)| v) }
            }
            ModelPattern::RuleCall {
                rule_path, args, ..
            } => self.generate_rule_call_parser(rule_path, args, is_lexical),
            ModelPattern::Lit { lit, .. } => {
                // Pure literal, no ws wrapping
                match lit {
                    syn::Lit::Str(_) => {
                        quote_spanned! {span=>
                            literal(#lit)
                                .context(::winnow::error::StrContext::Expected(::winnow::error::StrContextValue::StringLiteral(#lit)))
                        }
                    }
                    syn::Lit::Char(_) => {
                        quote_spanned! {span=>
                            literal(#lit)
                                .context(::winnow::error::StrContext::Expected(::winnow::error::StrContextValue::CharLiteral(#lit)))
                        }
                    }
                    _ => quote_spanned! {span=> literal(#lit) },
                }
            }
            ModelPattern::Group { alts, .. } => {
                if alts.len() == 1 {
                    self.generate_sequence_parser(&alts[0].0, is_lexical)
                } else {
                    let alts: Vec<TokenStream> = alts
                        .iter()
                        .map(|(seq, _, _)| self.generate_sequence_parser(seq, is_lexical))
                        .collect();
                    quote_spanned! {span=> alt(( #(#alts),* )) }
                }
            }
            ModelPattern::Optional(inner, _) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                quote_spanned! {span=> opt(#p) }
            }
            ModelPattern::Repeat(inner, _span) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                if !is_lexical {
                    if is_discarded {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, (), _, _>(0.., ::winnow::combinator::preceded(WS, #p)) }
                    } else {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(0.., ::winnow::combinator::preceded(WS, #p)) }
                    }
                } else if is_discarded {
                    quote_spanned! {span=> repeat::<_, _, (), _, _>(0.., #p) }
                } else {
                    quote_spanned! {span=> repeat::<_, _, Vec<_>, _, _>(0.., #p) }
                }
            }
            ModelPattern::Plus(inner, _span) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                if !is_lexical {
                    if is_discarded {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, (), _, _>(1.., ::winnow::combinator::preceded(WS, #p)) }
                    } else {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(1.., ::winnow::combinator::preceded(WS, #p)) }
                    }
                } else if is_discarded {
                    quote_spanned! {span=> repeat::<_, _, (), _, _>(1.., #p) }
                } else {
                    quote_spanned! {span=> repeat::<_, _, Vec<_>, _, _>(1.., #p) }
                }
            }
            ModelPattern::Parenthesized(inner, _) => {
                self.generate_delimited_expr(inner, "(", ")", is_lexical)
            }
            ModelPattern::Bracketed(inner, _) => {
                self.generate_delimited_expr(inner, "[", "]", is_lexical)
            }
            ModelPattern::Braced(inner, _) => {
                self.generate_delimited_expr(inner, "{", "}", is_lexical)
            }
            ModelPattern::Cut(_) => quote_spanned! {span=> ::winnow::combinator::empty },
            ModelPattern::Recover { body, sync, .. } => {
                let body_parser = self.generate_parser_expr(body, is_lexical, false);
                let sync_parser = self.generate_parser_expr(sync, is_lexical, false);
                quote_spanned! {span=>
                    alt((
                        #body_parser.map(Some),
                        (
                            ::winnow::combinator::repeat::<_, _, (), _, _>(0.., (
                                ::winnow::combinator::not(::winnow::combinator::peek(#sync_parser)),
                                ::winnow::token::any
                            )),
                            #sync_parser
                        ).map(|_| None)
                    ))
                }
            }
            ModelPattern::Peek(inner, _) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                quote_spanned! {span=> ::winnow::combinator::peek(#p) }
            }
            ModelPattern::Not(inner, _) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                quote_spanned! {span=> ::winnow::combinator::not(#p) }
            }
            ModelPattern::Until { pattern, .. } => {
                let p = self.generate_parser_expr(pattern, is_lexical, false);
                quote_spanned! {span=>
                     ::winnow::combinator::repeat::<_, _, (), _, _>(0.., (
                        ::winnow::combinator::not(::winnow::combinator::peek(#p)),
                        ::winnow::token::any
                    ))
                }
            }
            ModelPattern::Count { pattern, .. } => {
                let p = self.generate_parser_expr(pattern, is_lexical, false);
                if !is_lexical {
                    quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(0.., ::winnow::combinator::preceded(WS, #p)).map(|v: Vec<_>| v.len()) }
                } else {
                    quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(0.., #p).map(|v: Vec<_>| v.len()) }
                }
            }
            ModelPattern::Fail { message, .. } => match message {
                Some(msg) => {
                    quote_spanned! {span=> ::winnow::combinator::fail.context(::winnow::error::StrContext::Label(#msg)) }
                }
                None => quote_spanned! {span=> ::winnow::combinator::fail },
            },
            ModelPattern::LexicalScope(inner, _) => {
                // Lexical block implies strict parsing.
                // It does NOT consume whitespace before it starts (unless in a sequence where previous element added it).
                self.generate_parser_expr(inner, true, is_discarded)
            }
            ModelPattern::SpacedScope(inner, _) => {
                // Spaced block implies loose parsing.
                self.generate_parser_expr(inner, false, is_discarded)
            }
        }
    }

    fn generate_sequence_parser(&self, seq: &[ModelPattern], is_lexical: bool) -> TokenStream {
        let span = Span::mixed_site();
        let mut parsers = Vec::new();
        let mut in_cut = false;

        for (i, p) in seq.iter().enumerate() {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
                if i > 0 && !is_lexical {
                    parsers.push(quote_spanned! {span=> WS });
                }
                let p_expr = self.generate_parser_expr(p, is_lexical, false);
                if in_cut {
                    parsers.push(quote_spanned! {span=> ::winnow::combinator::cut_err(#p_expr) });
                } else {
                    parsers.push(p_expr);
                }
                continue;
            }

            // Infix WS
            if i > 0 && !is_lexical {
                parsers.push(quote_spanned! {span=> WS });
            }

            let p_expr = self.generate_parser_expr(p, is_lexical, false);
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
        is_lexical: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let inner_parser = self.generate_sequence_parser(inner, is_lexical);

        let open_p = quote_spanned! {span=> literal(#open) };
        let close_p = quote_spanned! {span=> literal(#close) };

        if is_lexical {
            quote_spanned! {span=>
                delimited(#open_p, #inner_parser, #close_p)
            }
        } else {
            // Infix logic: Open, WS, Inner, WS, Close
            // This is: delimited(open, preceded(WS, inner), preceded(WS, close))
            quote_spanned! {span=>
                delimited(#open_p, preceded(WS, #inner_parser), preceded(WS, #close_p))
            }
        }
    }
}

fn get_inner_binding(pattern: &ModelPattern) -> Option<&syn::Ident> {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => binding.as_ref(),
        ModelPattern::Group { alts, binding, .. } => {
            if let Some(b) = binding {
                return Some(b);
            }
            if alts.len() == 1 && alts[0].0.len() == 1 {
                get_inner_binding(&alts[0].0[0])
            } else {
                None
            }
        }
        ModelPattern::Lit { binding, .. } => binding.as_ref(),
        ModelPattern::Optional(inner, _) => get_inner_binding(inner),
        ModelPattern::Repeat(inner, _) => get_inner_binding(inner),
        ModelPattern::Plus(inner, _) => get_inner_binding(inner),
        ModelPattern::SpanBinding(inner, _, _) => get_inner_binding(inner),
        ModelPattern::Recover { binding, .. } => binding.as_ref(),
        ModelPattern::Until { binding, .. } => binding.as_ref(),
        ModelPattern::Count { binding, .. } => binding.as_ref(),
        ModelPattern::Parenthesized(inner, _)
        | ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _) => {
            if inner.len() == 1 {
                get_inner_binding(&inner[0])
            } else {
                None
            }
        }
        ModelPattern::LexicalScope(inner, _) | ModelPattern::SpacedScope(inner, _) => {
            get_inner_binding(inner)
        }
        _ => None,
    }
}
