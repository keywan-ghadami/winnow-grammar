//! `parse_<rule>_reader` for a `#[stream]` rule (ADR 27 §3).
//!
//! The path from the stream rule to its fold is cut into steps, each a
//! function generic over the input's lifetime and the error type like every
//! rule's: a **prefix** per rule (its elements before the call the path
//! continues in, returning what crosses and the arguments of that call), an
//! **item** (one element of the fold), and a **suffix** per rule (its
//! elements after, returning what its action needs). The reader function
//! runs them through `rt::stream::Stream` in order, the fold's loop and every
//! action of the path in between, where no view into the input is alive.

use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use winnow_grammar_model::model::{ModelPattern, Rule};
use winnow_grammar_model::stream::{Level, StreamPath};

impl Codegen<'_> {
    pub fn generate_stream_fns(&self, rule: &Rule) -> TokenStream {
        let name = rule.name.to_string();
        let Some(path) = self.streams.paths.get(&name) else {
            return quote! {};
        };
        let span = Span::mixed_site();
        let state_bound = self.state_bound();
        let input_ty = self.input_ty();
        let vis = if rule.is_pub {
            quote! { pub }
        } else {
            quote! {}
        };
        let (kind, view) = if self.is_bytes() {
            (
                quote! { ::winnow_grammar::rt::stream::Bytes },
                quote! { ::winnow_grammar::ParseBytes<'_, S> },
            )
        } else {
            (
                quote! { ::winnow_grammar::rt::stream::Text },
                quote! { ::winnow_grammar::ParseInput<'_, S> },
            )
        };
        let k = self.edge_k;
        let reader_fn = format_ident!("parse_{}_reader", rule.name, span = span);
        let ret = &rule.return_type;
        let params = rule.params.iter().map(|p| {
            let n = &p.name;
            let t =
                p.ty.as_ref()
                    .expect("a stream rule's parameters are values");
            quote! { #n: #t }
        });
        let end_fn = format_ident!("__stream_{}_end", rule.name, span = span);
        let rt_error = self.rt_error();
        let mut helpers = quote_spanned! {span=>
            #[allow(dead_code, non_snake_case)]
            fn #end_fn<'a, S: ::std::fmt::Debug + Clone #state_bound, E: #rt_error>(
                input: &mut #input_ty,
            ) -> ::winnow::Result<(), ::winnow::error::ErrMode<E>> {
                ::winnow_grammar::rt::skip_trivia(WS, input)
            }
        };
        let body = self.stream_level(path, 0, &view, &mut helpers);
        let call = |f: &syn::Ident, args: TokenStream| {
            quote_spanned! {span=>
                |input: &mut #view, pass: &::winnow_grammar::rt::stream::Pass| {
                    if pass.diagnose() {
                        #f::<S, ::winnow_grammar::ParseError>(input, #args)
                            .map_err(::winnow_grammar::rt::stream::Fail::from)
                    } else {
                        #f::<S, ::winnow::error::EmptyError>(input, #args)
                            .map_err(::winnow_grammar::rt::stream::Fail::from)
                    }
                }
            }
        };
        let finish = call(&end_fn, quote! {});
        quote_spanned! {span=>
            #helpers

            /// Parses what `reader` yields, one item of the rule's `fold` at
            /// a time, in a buffer of at most `reading.max_frame` bytes past
            /// the start of each item, plus a block (ADR 27 §3). The answer
            /// is the parse of the whole input in memory; errors are at their
            /// offset in the stream. `context` is the parse's: its user state
            /// and its recoveries are there when the call returns.
            #[allow(dead_code, non_snake_case, unused_mut, unused_variables, clippy::let_unit_value)]
            #vis fn #reader_fn<S: ::std::fmt::Debug + Clone #state_bound>(
                reader: impl ::std::io::Read,
                context: &mut ::winnow_grammar::ParseContext<S>,
                reading: ::winnow_grammar::rt::Reading,
                #(#params),*
            ) -> ::core::result::Result<#ret, ::winnow_grammar::ParseError> {
                let mut __stream = ::winnow_grammar::rt::stream::Stream::<#kind, _, S>::new(
                    reader, context, reading, #k,
                );
                let __value = #body;
                __stream.finish(#finish)?;
                ::core::result::Result::Ok(__value)
            }
        }
    }

    /// The code of level `j` of the path, as a block whose value is the
    /// value of the rule; its step functions go to `helpers`.
    fn stream_level(
        &self,
        path: &StreamPath,
        j: usize,
        view: &TokenStream,
        helpers: &mut TokenStream,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let level: &Level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let stream_rule = &path.levels[0].rule;
        let v = &rule.variants[0];
        let at = level.at;
        let lexical = rule.is_lexical;
        let input_ty = self.input_ty();
        let rt_error = self.rt_error();
        let state_bound = self.state_bound();
        let input = &self.input_ident;
        let rules: Vec<&str> = path.levels[..=j].iter().map(|l| l.rule.as_str()).collect();
        let skip = quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(WS, #input)?; };

        let params: Vec<(syn::Ident, syn::Type)> = rule
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone().expect("value parameter")))
            .collect();
        let typed = |vals: &[(syn::Ident, syn::Type)]| {
            vals.iter()
                .map(|(n, t)| quote! { #n: #t })
                .collect::<Vec<_>>()
        };
        let names = |vals: &[(syn::Ident, syn::Type)]| {
            vals.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
        };
        let clones = |vals: &[(syn::Ident, syn::Type)]| {
            vals.iter()
                .map(|(n, _)| quote! { ::core::clone::Clone::clone(&#n) })
                .collect::<Vec<_>>()
        };
        let types = |vals: &[(syn::Ident, syn::Type)]| {
            vals.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>()
        };
        let call = |f: &syn::Ident, args: &[TokenStream], ret: Option<TokenStream>| {
            let ret = ret.map(
                |r| quote! { -> ::core::result::Result<#r, ::winnow_grammar::rt::stream::Fail> },
            );
            quote_spanned! {span=>
                |input: &mut #view, pass: &::winnow_grammar::rt::stream::Pass| #ret {
                    if pass.diagnose() {
                        #f::<S, ::winnow_grammar::ParseError>(input, #(#args),*)
                            .map_err(::winnow_grammar::rt::stream::Fail::from)
                    } else {
                        #f::<S, ::winnow::error::EmptyError>(input, #(#args),*)
                            .map_err(::winnow_grammar::rt::stream::Fail::from)
                    }
                }
            }
        };

        // --- The prefix: the entry point's skip at the top, the rule's own
        // at its start, its elements before `at`, and the skip before the
        // element at `at`.
        let prefix_fn = format_ident!("__stream_{}_{}_prefix", stream_rule, j, span = span);
        let mut steps = TokenStream::new();
        if j == 0 {
            steps.extend(skip.clone());
        }
        if !lexical {
            steps.extend(skip.clone());
        }
        steps.extend(self.generate_sequence_steps(&v.pattern[..at], false, lexical));
        if !lexical && at > 0 {
            steps.extend(skip.clone());
        }
        let next_params: Vec<(syn::Ident, syn::Type)> = match path.levels.get(j + 1) {
            Some(next) => self
                .rule(&next.rule)
                .params
                .iter()
                .map(|p| (p.name.clone(), p.ty.clone().expect("value parameter")))
                .collect(),
            None => Vec::new(),
        };
        let next_args: Vec<TokenStream> = match &v.pattern[at] {
            ModelPattern::RuleCall { args, .. } if j + 1 < path.levels.len() => args
                .iter()
                .map(|a| self.generate_argument_expr(a, lexical))
                .collect(),
            _ => Vec::new(),
        };
        let carried = &level.carried;
        let (c_names, c_types) = (names(carried), types(carried));
        let a_types = types(&next_params);
        let p_typed = typed(&params);
        helpers.extend(quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_variables, unused_mut, clippy::let_unit_value)]
            fn #prefix_fn<'a, S: ::std::fmt::Debug + Clone #state_bound, E: #rt_error>(
                #input: &mut #input_ty,
                #(#p_typed),*
            ) -> ::winnow::Result<((#(#c_types,)*), (#(#a_types,)*)), ::winnow::error::ErrMode<E>> {
                use ::winnow::Parser;
                use ::winnow::prelude::*;
                #steps
                ::core::result::Result::Ok(((#(#c_names,)*), (#(#next_args,)*)))
            }
        });
        let a_names = names(&next_params);
        let prefix_call = call(&prefix_fn, &clones(&params), None);
        let prefix = quote_spanned! {span=>
            let ((#(#c_names,)*), (#(#a_names,)*)) = __stream.chunk(&[#(#rules),*], #prefix_call)?;
        };

        // --- The element at `at`: the next rule's level, or the fold.
        let (middle, middle_value) = match path.levels.get(j + 1) {
            Some(next) => {
                let inner = self.stream_level(path, j + 1, view, helpers);
                let ty = self.rule(&next.rule).return_type.clone();
                (quote_spanned! {span=> let __path: #ty = #inner; }, Some(ty))
            }
            None => (self.stream_fold(path, j, view, helpers), None),
        };
        let (binding, path_param) = match &v.pattern[at] {
            ModelPattern::RuleCall { binding, .. } => (binding.clone(), middle_value),
            ModelPattern::Fold { binding, .. } => (binding.clone(), None),
            _ => (None, None),
        };
        // The value at `at` under its name: for the suffix of a rule that
        // continues in a call, and for the action.
        let bind_path = match &binding {
            Some(b) => quote_spanned! {span=> let #b = __path; },
            None => quote! {},
        };

        // --- The suffix: its elements, after the skip before the first.
        let suffix_fn = format_ident!("__stream_{}_{}_suffix", stream_rule, j, span = span);
        let suffix_pats = &v.pattern[at + 1..];
        let cut = v.pattern[..=at]
            .iter()
            .any(|p| matches!(p, ModelPattern::Cut(_)));
        let mut steps = TokenStream::new();
        if !lexical && !suffix_pats.is_empty() {
            steps.extend(skip.clone());
        }
        steps.extend(self.generate_sequence_steps(suffix_pats, cut, lexical));
        let returned = &level.returned;
        let (r_names, r_types) = (names(returned), types(returned));
        let mut s_in = params.clone();
        s_in.extend(carried.iter().cloned());
        if let (Some(b), Some(ty)) = (&binding, &path_param) {
            s_in.push((b.clone(), ty.clone()));
        }
        let s_typed = typed(&s_in);
        helpers.extend(quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_variables, unused_mut, clippy::let_unit_value)]
            fn #suffix_fn<'a, S: ::std::fmt::Debug + Clone #state_bound, E: #rt_error>(
                #input: &mut #input_ty,
                #(#s_typed),*
            ) -> ::winnow::Result<(#(#r_types,)*), ::winnow::error::ErrMode<E>> {
                use ::winnow::Parser;
                use ::winnow::prelude::*;
                #steps
                ::core::result::Result::Ok((#(#r_names,)*))
            }
        });
        let suffix_call = call(&suffix_fn, &clones(&s_in), None);
        let suffix = quote_spanned! {span=>
            let (#(#r_names,)*) = __stream.chunk(&[#(#rules),*], #suffix_call)?;
        };

        // --- The action, between steps, with the context.
        let ret = &rule.return_type;
        let action = &v.action;
        let action = if v.is_explicit {
            quote! { { #action } }
        } else {
            quote! { #action }
        };
        quote_spanned! {span=>
            {
                #prefix
                #middle
                #bind_path
                #suffix
                let _state = __stream.context();
                let __r: ::winnow::Result<#ret, ::winnow::error::ErrMode<::winnow_grammar::ParseError>> =
                    (|| ::core::result::Result::Ok(#action))();
                match __r {
                    ::core::result::Result::Ok(v) => v,
                    ::core::result::Result::Err(e) => {
                        return ::core::result::Result::Err(::winnow_grammar::rt::stream::action_error(e))
                    }
                }
            }
        }
    }

    /// The fold of the last rule of the path: `__path` is its value.
    fn stream_fold(
        &self,
        path: &StreamPath,
        j: usize,
        view: &TokenStream,
        helpers: &mut TokenStream,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let stream_rule = &path.levels[0].rule;
        let lexical = rule.is_lexical;
        let input = &self.input_ident;
        let input_ty = self.input_ty();
        let rt_error = self.rt_error();
        let state_bound = self.state_bound();
        let rules: Vec<&str> = path.levels[..=j].iter().map(|l| l.rule.as_str()).collect();
        let ModelPattern::Fold {
            pattern,
            init,
            step,
            ..
        } = &rule.variants[0].pattern[level.at]
        else {
            unreachable!("the model checked that the path ends in a fold")
        };
        let ModelPattern::RuleCall {
            rule_path, args, ..
        } = &**pattern
        else {
            unreachable!("the model checked that a streamed fold's item is a rule call")
        };
        let item_rule = self.rule(&rule_path.segments.last().unwrap().ident.to_string());
        let item_ret = &item_rule.return_type;
        let item_inner = format_ident!("parse_{}_inner", item_rule.name, span = span);
        let item_fn = format_ident!("__stream_{}_item", stream_rule, span = span);
        let step_fn = format_ident!("__stream_{}_step", stream_rule, span = span);
        let a_names: Vec<syn::Ident> = (0..item_rule.params.len())
            .map(|i| format_ident!("__a{}", i, span = span))
            .collect();
        let a_types: Vec<&syn::Type> = item_rule
            .params
            .iter()
            .map(|p| p.ty.as_ref().expect("value parameter"))
            .collect();
        let skip = if lexical {
            quote! {}
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(WS, #input)?; }
        };
        let item_parser = self.edge(
            quote_spanned! {span=>
                (|i: &mut #input_ty| #item_inner(i, #(::core::clone::Clone::clone(&#a_names)),*))
            },
            Some(pattern),
        );
        // A step whose accumulator is written `&mut` changes it in place, as
        // in the fold everywhere else (ADR 24 §11).
        let in_place = matches!(
            &**step,
            syn::Expr::Closure(c) if matches!(
                c.inputs.first(),
                Some(syn::Pat::Type(t)) if matches!(&*t.ty, syn::Type::Reference(r) if r.mutability.is_some())
            )
        );
        let step_bound = if in_place {
            quote! { for<'a> FnMut(&mut A, #item_ret) }
        } else {
            quote! { for<'a> FnMut(A, #item_ret) -> A }
        };
        helpers.extend(quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_variables, unused_mut)]
            fn #item_fn<'a, S: ::std::fmt::Debug + Clone #state_bound, E: #rt_error>(
                #input: &mut #input_ty,
                #(#a_names: #a_types),*
            ) -> ::winnow::Result<#item_ret, ::winnow::error::ErrMode<E>> {
                use ::winnow::Parser;
                #skip
                let mut __p = #item_parser;
                ::winnow::Parser::parse_next(&mut __p, #input)
            }

            /// Makes the step take an item of any input's lifetime: the
            /// items of a stream come from many buffers.
            #[allow(dead_code, non_snake_case)]
            fn #step_fn<A, F: #step_bound>(_: &::core::option::Option<A>, f: F) -> F {
                f
            }
        });
        let args: Vec<TokenStream> = args
            .iter()
            .map(|a| self.generate_argument_expr(a, lexical))
            .collect();
        let apply = if in_place {
            quote_spanned! {span=>
                __step(__acc.as_mut().expect("the accumulator is there"), __v);
            }
        } else {
            quote_spanned! {span=>
                let __a = __acc.take().expect("the accumulator is there");
                __acc = ::core::option::Option::Some(__step(__a, __v));
            }
        };
        quote_spanned! {span=>
            let mut __acc = ::core::option::Option::Some((#init)());
            let mut __step = #step_fn(&__acc, #step);
            let mut __n = 0usize;
            while __stream.items(&[#(#rules),*], &mut __n, |input: &mut #view, pass: &::winnow_grammar::rt::stream::Pass| {
                let __v = if pass.diagnose() {
                    #item_fn::<S, ::winnow_grammar::ParseError>(input, #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                } else {
                    #item_fn::<S, ::winnow::error::EmptyError>(input, #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                };
                if ::winnow_grammar::rt::stream::commit(input, pass) {
                    #apply
                    ::core::result::Result::Ok(true)
                } else {
                    ::core::result::Result::Ok(false)
                }
            })? {}
            let __path = __acc.take().expect("the accumulator is there");
        }
    }

    fn rule(&self, name: &str) -> &Rule {
        self.grammar
            .rules
            .iter()
            .find(|r| r.name == name)
            .expect("the model checked the path")
    }
}
