use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use winnow_grammar_model::{
    analysis,
    model::{Rule, RuleVariant},
};

impl<'a> Codegen<'a> {
    pub fn generate_rule(&self, rule: &Rule) -> TokenStream {
        let state_bound = self.state_bound();
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
        // The public parser reports `ParseError`; inside, the rule is generic
        // over its error type `E` (ADR 17): the entry point instantiates it
        // twice, once with `EmptyError` for the fast pass and once with
        // `ParseError` for the diagnosing one.
        let err_type = quote_spanned! { span=> ::winnow_grammar::ParseError };
        let inner_err_type = quote_spanned! { span=> ::winnow::error::ErrMode<E> };

        let mut params_tokens = Vec::new();
        let mut inner_params_tokens = Vec::new();
        let mut arg_names = Vec::new();

        // Only value parameters get here: a rule with a parser-typed or an
        // untyped parameter is a template (`is_template`) and is inlined at
        // its call sites instead of compiled to a function.
        for param in &rule.params {
            let name = &param.name;
            let ty = &param.ty;
            if let Some(t) = ty {
                params_tokens.push(quote! { mut #name: #t });
                inner_params_tokens.push(quote! { #name: #t });
                arg_names.push(quote! { #name.clone() });
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

        let mut all_generics = quote! { 'a, S: std::fmt::Debug + Clone #state_bound, E: ::winnow_grammar::rt::RtError<'a, S> };
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
                fn WS<'a, S: std::fmt::Debug + Clone, E>(_: &mut ::winnow_grammar::ParseInput<'a, S>) -> ::winnow::Result<(), ::winnow::error::ErrMode<E>> {
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
                // Errors passed out collect it via `.context(Label)`. The fast
                // pass records nothing and keeps no stack.
                if <E as ::winnow_grammar::Diagnostics>::RECORDING {
                    #input.state.rules.push(#rule_name_str);
                }
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
                if <E as ::winnow_grammar::Diagnostics>::RECORDING {
                    #input.state.rules.pop();
                }
                result
            }
        };

        let mut outer_generics = quote! {'a, S: std::fmt::Debug + Clone #state_bound };
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
            (
                quote! { ::winnow_grammar::rt::skip_trivia(WS, input)?; },
                quote! { ::winnow_grammar::rt::skip_trivia(WS, input)?; },
            )
        };

        // The entry point runs the rule twice at most: a fast pass with
        // `EmptyError`, and - only if that fails - the diagnosing pass with
        // `ParseError`, whose error is the one reported (ADR 17). The typed
        // `let` names the error type of each pass; a turbofish could not,
        // because it would have to spell every generic of the rule.
        let fast_err =
            quote_spanned! {span=> ::winnow::error::ErrMode<::winnow::error::EmptyError> };
        let slow_err = quote_spanned! {span=> ::winnow::error::ErrMode<#err_type> };
        let pass = |err: &TokenStream| {
            quote! {
                |input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err> {
                    #ws_before
                    let result: ::winnow::Result<#ret_type, #err> = #inner_fn_name(input, #(#arg_names),*);
                    let result = result?;
                    #ws_after
                    Ok(result)
                }
            }
        };
        let fast = pass(&fast_err);
        let slow = pass(&slow_err);
        // A `par_fold` rule replays from the item the fast pass stopped in.
        let entry = if is_par_fold {
            quote_spanned! {span=> ::winnow_grammar::rt::entry_framed }
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::entry }
        };

        let outer_fn_body = quote! {
            move |input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                #entry(input, #fast, #slow)
            }
        };

        // Outer function signature doesn't need to specify for<'a> if it already uses 'a in its signature
        // A doc comment on a rule belongs on the parser it generates; the
        // validator rejects every other attribute, so this is the whole set.
        let docs = rule
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("doc"))
            .collect::<Vec<_>>();

        let outer_fn = quote_spanned! {span=>
            #(#docs)*
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

    /// The functions for parsing in pieces, next to the rule's parser:
    ///
    /// * on a `#[frame]` rule and on a `par_fold` rule,
    ///   `frames_<rule>(input, n)`: the byte ranges of `n` pieces, each
    ///   starting at a boundary (`rt::frames`);
    /// * on a `par_fold` rule, `merge_<rule>(a, b)`, the merge it was given,
    ///   and `parse_<rule>_pieces(input, new_context, how)`, the driver: it
    ///   cuts, parses every piece with `parse_<rule>()`, merges, and reports a
    ///   piece's error at its position in the whole input. `how` is a
    ///   `rt::Parallelism`: off, a number of pieces, or one per core; with
    ///   the `rayon` feature the pieces run in parallel, without it in
    ///   sequence - same cut, same answer.
    fn generate_frame_fns(&self, rule: &Rule) -> TokenStream {
        let state_bound = self.state_bound();
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
        let driver = match merge {
            Some(m) => {
                let merge_fn = format_ident!("merge_{}", rule.name, span = span);
                let pieces_fn = format_ident!("parse_{}_pieces", rule.name, span = span);
                let pieces_with_fn = format_ident!("parse_{}_pieces_with", rule.name, span = span);
                let inner_fn = format_ident!("parse_{}_inner", rule.name, span = span);
                let ret_type = &rule.return_type;
                quote_spanned! {span=>
                    /// The merge given to `par_fold`: combines the results of
                    /// two pieces.
                    #[allow(dead_code)]
                    #vis fn #merge_fn<'a>(a: #ret_type, b: #ret_type) -> #ret_type {
                        let mut merge = #m;
                        merge(a, b)
                    }

                    /// Parses `input` in pieces and merges: the cut is
                    /// `frames_…`, the per-piece parser the rule's own (fast
                    /// pass, then the diagnosing replay from the failing item -
                    /// ADR 17), the merge `merge_…`. A piece's error is
                    /// reported at its offset in `input`.
                    ///
                    /// Every piece parses with a clone of `context`, so the
                    /// interner inside it is *shared* - symbols from two
                    /// pieces mean the same thing, which is what ADR 14 asks
                    /// for and what nothing used to do. A `user_state` that
                    /// must start empty per piece instead of being copied
                    /// wants `parse_…_pieces_with` - ADR 19.
                    #[allow(dead_code)]
                    #vis fn #pieces_fn<'a, S: std::fmt::Debug + Clone #state_bound>(
                        input: &'a str,
                        context: &::winnow_grammar::ParseContext<S>,
                        how: ::winnow_grammar::rt::Parallelism,
                    ) -> ::core::result::Result<#ret_type, ::winnow_grammar::ParseError>
                    where
                        S: ::std::fmt::Debug + Clone + Sync,
                        #ret_type: Send,
                    {
                        #pieces_with_fn(input, || context.clone(), how)
                    }

                    /// [`#pieces_fn`] with a context *built* per piece rather
                    /// than cloned: for a `user_state` that has to start empty
                    /// in every piece - a table whose slots are the piece's
                    /// own, an accumulator that must not be copied.
                    ///
                    /// The closure is where an interner is shared, by cloning
                    /// one into each context; a closure that builds a fresh
                    /// interner gives every piece its own numbering, and
                    /// symbols from two pieces are then not comparable.
                    #[allow(dead_code)]
                    #vis fn #pieces_with_fn<'a, S: std::fmt::Debug + Clone #state_bound>(
                        input: &'a str,
                        new_context: impl Fn() -> ::winnow_grammar::ParseContext<S> + Sync,
                        how: ::winnow_grammar::rt::Parallelism,
                    ) -> ::core::result::Result<#ret_type, ::winnow_grammar::ParseError>
                    where
                        S: ::std::fmt::Debug + Clone,
                        #ret_type: Send,
                    {
                        ::winnow_grammar::rt::fold_pieces(
                            input,
                            #boundary,
                            how,
                            new_context,
                            |piece: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, ::winnow::error::ErrMode<::winnow::error::EmptyError>> {
                                #inner_fn(piece)
                            },
                            |piece: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, ::winnow::error::ErrMode<::winnow_grammar::ParseError>> {
                                #inner_fn(piece)
                            },
                            #merge_fn,
                        )
                    }
                }
            }
            None => quote! {},
        };

        quote! {
            #frames
            #driver
        }
    }
}
