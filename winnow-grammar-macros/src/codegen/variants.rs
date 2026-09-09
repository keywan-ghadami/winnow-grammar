use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use winnow_grammar_model::model::{ModelPattern, RuleVariant};

impl<'a> Codegen<'a> {
    pub fn generate_variants_body(
        &self,
        variants: &[RuleVariant],
        ret_type: &syn::Type,
        is_lexical: bool,
        is_rule_start: bool,
        label: Option<&str>,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;

        // A rule-level `# "…"` substitutes for the expectations of *every*
        // alternative, and only when the rule failed where it began. So the
        // whitespace a syntactic rule skips at its start has to happen outside
        // the label: skipped inside, the failure sits past the blank, the
        // offsets no longer match and the label never substitutes. Hoisted, it
        // is also done once instead of once per alternative.
        let skip_inside = is_rule_start && label.is_none();

        let variant_parsers = variants.iter().map(|v| {
            let mut steps_code = TokenStream::new();
            let use_with_span = v.with_span;
            let is_explicit = v.is_explicit;

            // 1. Optional Leading WS
            if skip_inside && !is_lexical {
                steps_code.extend(quote! { ::winnow_grammar::rt::skip_trivia(WS, #input)?; });
            }

            // 2. Capture Start
            if use_with_span {
                steps_code
                    .extend(quote! { let start = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; });
            }

            // 3. Parse Steps
            let sequence_steps = self.generate_sequence_steps(&v.pattern, false, is_lexical);
            steps_code.extend(sequence_steps);

            // 4. Capture End and Define _span
            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; });
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            // Every action gets the context, whether or not it uses it: the
            // `_` prefix silences the unused binding, and the alternative -
            // searching the action's token text for `_state` - fired on the
            // word inside a string literal or a comment and made the binding's
            // name a hidden part of the API. `&mut` so that an action can both
            // read the context and intern into it (ADR 18 §2). `Stateful`
            // keeps its state in a public field; it has no `state_mut()`.
            let state_injection = quote! { let _state = &mut #input.state; };

            let final_expr = if use_with_span && !is_explicit {
                // Implicit action -> use WithSpan
                quote! {
                    Ok(<#ret_type as ::winnow_grammar::WithSpan<_>>::with_span({ #action }, _span))
                }
            } else {
                // Explicit action (user handles _span if needed) OR no span requested
                quote! {
                    Ok({ #action })
                }
            };

            let closure = quote_spanned! {span=>
                |#input: &mut ::winnow_grammar::ParseInput<'a, S>| {
                    #steps_code
                    #state_injection
                    #final_expr
                }
            };
            // `# "…"`: if the alternative fails at its starting position, its
            // name counts as the expectation. Until now the label was parsed
            // and discarded.
            match &v.label {
                Some(label) => quote_spanned! {span=> ::winnow_grammar::rt::labelled(#label, #closure) },
                None => closure,
            }
        });

        if variants.len() == 1 {
            let v = &variants[0];
            let mut steps_code = TokenStream::new();
            let use_with_span = v.with_span;
            let is_explicit = v.is_explicit;

            if skip_inside && !is_lexical {
                steps_code.extend(quote! { ::winnow_grammar::rt::skip_trivia(WS, #input)?; });
            }

            if use_with_span {
                steps_code
                    .extend(quote! { let start = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; });
            }

            steps_code.extend(self.generate_sequence_steps(&v.pattern, false, is_lexical));

            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; });
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            // Every action gets the context, whether or not it uses it: the
            // `_` prefix silences the unused binding, and the alternative -
            // searching the action's token text for `_state` - fired on the
            // word inside a string literal or a comment and made the binding's
            // name a hidden part of the API. `&mut` so that an action can both
            // read the context and intern into it (ADR 18 §2). `Stateful`
            // keeps its state in a public field; it has no `state_mut()`.
            let state_injection = quote! { let _state = &mut #input.state; };

            let final_expr = if use_with_span && !is_explicit {
                quote! {
                    Ok(<#ret_type as ::winnow_grammar::WithSpan<_>>::with_span({ #action }, _span))
                }
            } else {
                // An action written by the user comes without its braces and
                // may contain statements (`-> { let x = …; x }`); it gets the
                // braces back here - previously its tokens ended up in
                // expression position ("expected expression, found `let`
                // statement"). An action synthesized by the parser is always a
                // single expression (`()`, a binding, a tuple) and stays
                // unbraced - `{ () }` would be a Clippy finding.
                if v.is_explicit {
                    quote! { Ok({ #action }) }
                } else {
                    quote! { Ok(#action) }
                }
            };

            let body = quote_spanned! {span=>
                {
                    #steps_code
                    #state_injection
                    #final_expr
                }
            };
            // A single-variant rule may be labelled too (`# "…"`): if it fails
            // at its starting position, its name is the expectation.
            let body = match &v.label {
                Some(label) => quote_spanned! {span=>
                    {
                        let mut __labelled = ::winnow_grammar::rt::labelled(
                            #label,
                            |#input: &mut ::winnow_grammar::ParseInput<'a, S>| #body,
                        );
                        ::winnow::Parser::parse_next(&mut __labelled, #input)
                    }
                },
                None => body,
            };
            self.wrap_in_rule_label(body, label, is_rule_start, is_lexical)
        } else {
            let body = quote_spanned! {span=>
                alt((
                    #(#variant_parsers),*
                )).parse_next(#input)
            };
            self.wrap_in_rule_label(body, label, is_rule_start, is_lexical)
        }
    }

    /// `rule primary_expr -> Expr # "expression" = a | b | …`
    ///
    /// The rule's own name for itself, reported when it fails at the position
    /// it started at - the case where listing what each alternative could have
    /// begun with says the least. If it got further, whatever it was in the
    /// middle of is the more informative message and stays.
    ///
    /// The leading whitespace skip is emitted here, outside the label, for the
    /// reason `skip_inside` gives.
    fn wrap_in_rule_label(
        &self,
        body: TokenStream,
        label: Option<&str>,
        is_rule_start: bool,
        is_lexical: bool,
    ) -> TokenStream {
        let Some(label) = label else {
            return body;
        };
        let span = Span::mixed_site();
        let input = &self.input_ident;
        let lead = if is_rule_start && !is_lexical {
            quote! { ::winnow_grammar::rt::skip_trivia(WS, #input)?; }
        } else {
            quote! {}
        };
        quote_spanned! {span=>
            {
                #lead
                let mut __rule_labelled = ::winnow_grammar::rt::labelled(
                    #label,
                    |#input: &mut ::winnow_grammar::ParseInput<'a, S>| { #body },
                );
                ::winnow::Parser::parse_next(&mut __rule_labelled, #input)
            }
        }
    }

    pub fn generate_recursive_loop_body(
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
                steps_code.extend(quote! { ::winnow_grammar::rt::skip_trivia(WS, #input)?; });
            }

            steps_code.extend(self.generate_sequence_steps(patterns, false, is_lexical));

            // Capture end and define _span
            if use_with_span {
                steps_code
                    .extend(quote! { let end = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; });
                // For recursive steps, _span refers to the suffix extension.
                steps_code.extend(quote! { #[allow(unused_variables)] let _span = start..end; });
            }

            let action = &v.action;
            // Every action gets the context, whether or not it uses it: the
            // `_` prefix silences the unused binding, and the alternative -
            // searching the action's token text for `_state` - fired on the
            // word inside a string literal or a comment and made the binding's
            // name a hidden part of the API. `&mut` so that an action can both
            // read the context and intern into it (ADR 18 §2). `Stateful`
            // keeps its state in a public field; it has no `state_mut()`.
            let state_injection = quote! { let _state = &mut #input.state; };

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
                     Ok(<#ret_type as ::winnow_grammar::WithSpan<_>>::with_span({ #action }, full_span))
                }
            } else {
                // An action written by the user comes without its braces and
                // may contain statements (`-> { let x = …; x }`); it gets the
                // braces back here - previously its tokens ended up in
                // expression position ("expected expression, found `let`
                // statement"). An action synthesized by the parser is always a
                // single expression (`()`, a binding, a tuple) and stays
                // unbraced - `{ () }` would be a Clippy finding.
                if v.is_explicit {
                    quote! { Ok({ #action }) }
                } else {
                    quote! { Ok(#action) }
                }
            };

            // Start capture needs to happen before steps_code
            // 'start' here refers to start of suffix
            let start_capture = if use_with_span {
                 quote! { let start = ::winnow::combinator::empty.with_span().parse_next(#input).map(|(_, s)| s.start)?; }
            } else {
                quote! {}
            };

            quote_spanned! {span=>
                {
                    let checkpoint = ::winnow::stream::Stream::checkpoint(#input);
                    #start_capture
                    let attempt = (|| {
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
                            if matches!(e, ::winnow::error::ErrMode::Cut(_)) {
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
}
