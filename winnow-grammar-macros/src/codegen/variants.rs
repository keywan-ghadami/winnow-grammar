use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn_grammar_model::model::{ModelPattern, RuleVariant};

impl<'a> Codegen<'a> {
    pub fn generate_variants_body(
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
                steps_code.extend(quote! { let _ = WS(#input)?; });
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
                |#input: &mut ::winnow_grammar::ParseInput<'a, S>| {
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
                steps_code.extend(quote! { let _ = WS(#input)?; });
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

    pub fn generate_recursive_loop_body(
        &self,
        variants: &[RuleVariant],
        _ret_type: &syn::Type,
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
                steps_code.extend(quote! { let _ = WS(#input)?; });
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
