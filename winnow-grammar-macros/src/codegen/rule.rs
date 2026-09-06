use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn;
use winnow_grammar_model::{
    analysis,
    model::{Rule, RuleVariant},
};

impl<'a> Codegen<'a> {
    pub fn generate_rule(&self, rule: &Rule) -> TokenStream {
        if super::is_template(rule) {
            // Template rules are not compiled as functions of their own.
            // They are inlined directly at the call site in expr.rs via AST substitution.
            return quote! {};
        }

        let rule_name = &rule.name;
        let rule_name_str = rule_name.to_string();
        let is_ws_rule = rule_name_str == "WS";
        *self.current_boundary.borrow_mut() =
            self.frames.boundary_for(&rule_name_str).map(str::to_owned);
        let span = Span::mixed_site();
        let fn_name = format_ident!("parse_{}", rule_name, span = span);
        let inner_fn_name = format_ident!("parse_{}_inner", rule_name, span = span);
        let ret_type = &rule.return_type;
        let input = &self.input_ident;
        let err_type = quote_spanned! { span=> ::winnow_grammar::ParseError };
        let inner_err_type = quote_spanned! { span=> ::winnow::error::ErrMode<#err_type> };

        let mut params_tokens = Vec::new();
        let mut inner_params_tokens = Vec::new();
        let mut arg_names = Vec::new();
        let mut extra_generics = Vec::<syn::Ident>::new();
        let mut param_wrappers = Vec::new();

        for param in &rule.params {
            let name = &param.name;
            let ty = &param.ty;

            match ty {
                Some(t) => {
                    let mut actual_ty = quote! { #t };
                    let mut is_parser = false;
                    if let syn::Type::Path(type_path) = t {
                        if let Some(segment) = type_path.path.segments.last() {
                            if segment.ident == "Rule" {
                                is_parser = true;
                                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                                {
                                    let inner_args = &args.args;
                                    actual_ty = quote! { impl ::winnow::Parser<::winnow_grammar::ParseInput<'a, S>, #inner_args, #err_type> };
                                }
                            }
                        }
                    }
                    params_tokens.push(quote! { mut #name: #actual_ty });
                    if is_parser {
                        let wrapper_name = format_ident!("{}_wrapper", name, span = span);
                        param_wrappers.push(quote_spanned! {span=>
                            let mut #wrapper_name = |i: &mut _| {
                                ::winnow::Parser::parse_next(&mut #name, i).map_err(::winnow::error::ErrMode::Backtrack)
                            };
                        });
                        inner_params_tokens.push(quote! { #wrapper_name: &mut impl ::winnow::Parser<::winnow_grammar::ParseInput<'a, S>, _, #inner_err_type> });
                        arg_names.push(quote! { &mut #wrapper_name });
                    } else {
                        inner_params_tokens.push(quote! { #name: #actual_ty });
                        arg_names.push(quote! { #name.clone() });
                    }
                }
                None => {
                    let output_type = format_ident!("Output_{}", name, span = Span::mixed_site());
                    extra_generics.push(output_type.clone());
                    let actual_ty = quote! { impl ::winnow::Parser<::winnow_grammar::ParseInput<'a, S>, #output_type, #err_type> };

                    params_tokens.push(quote! { mut #name: #actual_ty });
                    let wrapper_name = format_ident!("{}_wrapper", name, span = span);
                    param_wrappers.push(quote_spanned! {span=>
                        let mut #wrapper_name = |i: &mut _| {
                            ::winnow::Parser::parse_next(&mut #name, i).map_err(::winnow::error::ErrMode::Backtrack)
                        };
                    });
                    inner_params_tokens.push(quote! { #wrapper_name: &mut impl ::winnow::Parser<::winnow_grammar::ParseInput<'a, S>, #output_type, #inner_err_type> });
                    arg_names.push(quote! { &mut #wrapper_name });
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

        let mut all_generics = quote! { 'a, S: std::fmt::Debug + Clone };
        if !extra_generics.is_empty() {
            all_generics.extend(quote! {, #(#extra_generics),* });
        }
        if !gen_params.is_empty() {
            all_generics.extend(quote! {, #gen_params});
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
                fn WS<'a, S: std::fmt::Debug + Clone>(_: &mut ::winnow_grammar::ParseInput<'a, S>) -> ::winnow::Result<(), #inner_err_type> {
                    Ok(())
                }
            }
        } else {
            quote! {}
        };

        let inner_fn = quote_spanned! {span=>
            #[allow(dead_code)]
            fn #inner_fn_name<#all_generics>(#input: &mut ::winnow_grammar::ParseInput<'a, S>, #(#inner_params_tokens),*) -> ::winnow::Result<#ret_type, #inner_err_type>
            where
                #where_preds
            {
                use ::winnow::Parser;

                #ws_shadow

                let mut parser = (|#input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #inner_err_type> {
                    use ::winnow::prelude::*;
                    #body
                })
                .context(::winnow::error::StrContext::Label(#rule_name_str));

                // The rule name sits on the live stack for the duration of the
                // body, so that an error RECORDED along the way picks it up.
                // Errors passed out collect it via `.context(Label)`.
                #input.state.rules.push(#rule_name_str);
                let result = {
                    #[cfg(feature = "trace")]
                    {
                        ::winnow::combinator::trace(#rule_name_str, parser).parse_next(#input)
                    }
                    #[cfg(not(feature = "trace"))]
                    {
                        parser.parse_next(#input)
                    }
                };
                #input.state.rules.pop();
                result
            }
        };

        let mut outer_generics = quote! {'a, S: std::fmt::Debug + Clone };
        if !extra_generics.is_empty() {
            outer_generics.extend(quote! {, #(#extra_generics),* });
        }
        if !gen_params.is_empty() {
            outer_generics.extend(quote! {, #gen_params});
        }

        // The entry point tolerates whitespace around the input - except on a
        // `par_fold` rule. Its parser runs once per *piece* when the input is
        // parsed in pieces, and whitespace it skipped there would be skipped
        // at every piece start and end instead of once at the input's: a
        // frame beginning with a space would parse differently depending on
        // where the cut fell, and whitespace-only garbage between two frames
        // would be tolerated in the piece that happens to end there and
        // rejected in the sequential parse. Skipping nothing makes the two
        // agree on every input, which is the property `par_fold` promises.
        let is_par_fold = self.frames.par_folds.contains_key(&rule_name_str);
        let (ws_before, ws_after) = if is_par_fold {
            (quote! {}, quote! {})
        } else {
            (quote! { WS(input)?; }, quote! { WS(input)?; })
        };

        let outer_fn_body = quote! {
            move |input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                #(#param_wrappers)*

                // Entry point: the recorded error belongs to THIS run.
                input.state.furthest = None;

                let result: ::winnow::Result<#ret_type, ::winnow::error::ErrMode<#err_type>> = (|| {
                    #ws_before
                    let result = #inner_fn_name(input, #(#arg_names),*)?;
                    #ws_after
                    Ok(result)
                })();

                // Error selection against the recorded error and check for
                // leftover input - see `rt::finish`.
                ::winnow_grammar::rt::finish(input, result)
            }
        };

        // Outer function signature doesn't need to specify for<'a> if it already uses 'a in its signature
        let outer_fn = quote_spanned! {span=>
            #vis fn #fn_name<#outer_generics> (#(#params_tokens),*) -> impl ::winnow::Parser<
                ::winnow_grammar::ParseInput<'a, S>,
                #ret_type,
                #err_type
            >
            where
                #where_preds
            {
                #outer_fn_body
            }
        };

        let frame_fns = self.generate_frame_fns(rule);

        quote! {
            #inner_fn
            #outer_fn
            #frame_fns
        }
    }

    /// The functions a driver needs to parse in pieces, next to the rule's
    /// parser:
    ///
    /// * on a `#[frame]` rule, `frames_<rule>(input, n)` - the byte ranges of
    ///   `n` pieces, each starting at a boundary (`rt::frames`);
    /// * on a `par_fold` rule, the same `frames_<rule>` (for the frame it
    ///   folds over) and `merge_<rule>(a, b)`, the merge it was given.
    ///
    /// The per-piece parser is the rule's own `parse_<rule>()` applied to the
    /// piece; the fold matches zero or more frames, so a piece is a valid
    /// input on its own.
    fn generate_frame_fns(&self, rule: &Rule) -> TokenStream {
        let span = Span::mixed_site();
        let rule_name_str = rule.name.to_string();
        let vis = if rule.is_pub {
            quote! { pub }
        } else {
            quote! {}
        };
        let frames_fn = format_ident!("frames_{}", rule.name, span = span);

        let boundary = if let Some(b) = self.frames.frames.get(&rule_name_str) {
            Some(b.clone())
        } else {
            self.frames
                .par_folds
                .get(&rule_name_str)
                .and_then(|item| self.frames.frames.get(item))
                .cloned()
        };
        let Some(boundary) = boundary else {
            return quote! {};
        };

        let frames = quote_spanned! {span=>
            /// The byte ranges of `n` pieces of `input`, each beginning at a
            /// frame boundary, together covering the input. Parse each piece
            /// with the rule's parser and combine the results.
            #[allow(dead_code)]
            #vis fn #frames_fn(input: &str, n: usize) -> ::std::vec::Vec<::core::ops::Range<usize>> {
                ::winnow_grammar::rt::frames(input, #boundary, n)
            }
        };

        let merge = rule.variants.first().and_then(|v| match v.pattern.first() {
            Some(winnow_grammar_model::model::ModelPattern::Fold { merge: Some(m), .. }) => {
                Some(m.clone())
            }
            _ => None,
        });
        let merge = match merge {
            Some(m) => {
                let merge_fn = format_ident!("merge_{}", rule.name, span = span);
                let ret_type = &rule.return_type;
                quote_spanned! {span=>
                    /// The merge given to `par_fold`: combines the results of
                    /// two pieces.
                    #[allow(dead_code)]
                    #vis fn #merge_fn<'a>(a: #ret_type, b: #ret_type) -> #ret_type {
                        let mut merge = #m;
                        merge(a, b)
                    }
                }
            }
            None => quote! {},
        };

        quote! {
            #frames
            #merge
        }
    }
}
