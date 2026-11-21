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
use winnow_grammar_model::parser::FoldBounds;
use winnow_grammar_model::stream::{Level, StreamPath};

impl Codegen<'_> {
    pub fn generate_stream_fns(&self, rule: &Rule) -> TokenStream {
        let ws = self.ws();
        let name = rule.name.to_string();
        let Some(path) = self.streams.paths.get(&name) else {
            return quote! {};
        };
        let span = Span::mixed_site();
        let state_bound = self.state_bound();
        let input_ty = self.input_ty();
        let vis = &rule.vis;
        let (kind, view) = (self.stream_kind(), self.stream_view());
        let k = self.edge_k;
        let reach = self.reach;
        let reader_fn = format_ident!("parse_{}_reader", rule.name, span = span);
        let ret = &rule.return_type;
        let params: Vec<TokenStream> = rule
            .params
            .iter()
            .map(|p| {
                let n = &p.name;
                let t =
                    p.ty.as_ref()
                        .expect("a stream rule's parameters are values");
                quote! { #n: #t }
            })
            .collect();
        let names: Vec<&syn::Ident> = rule.params.iter().map(|p| &p.name).collect();
        let rt_error = self.rt_error();
        let end_def = |end_fn: &syn::Ident, ws: &syn::Ident| {
            quote_spanned! {span=>
                #[allow(dead_code, non_snake_case)]
                fn #end_fn<'a, __WgC: ::winnow_grammar::Ctx<State: ::std::fmt::Debug + Clone #state_bound>, __WgEr: #rt_error>(
                    input: &mut #input_ty,
                ) -> ::winnow::Result<(), ::winnow::error::ErrMode<__WgEr>> {
                    ::winnow_grammar::rt::skip_trivia(#ws, input)
                }
            }
        };
        let end_fn = self.step_name(format!("__stream_{}_end", rule.name));
        let mut helpers = end_def(&end_fn, &ws);
        // The reader's body: a prefix, a suffix and the end call their plain
        // copy under `Pass::last`, the fold's item does not (`plain_items`).
        let body = self.with_plain_calls(false, || {
            self.stream_level(path, 0, &view, &mut helpers, false)
        });
        // The slice entry's body: the same steps, calling the plain step
        // functions once the input is all there. The reader's body does not
        // call them, so that a program with the reader and the parse in
        // memory but no slice entry does not instantiate them: a plain rule
        // with a second caller is no longer taken into the fold in memory
        // (cut_profile `stmts_*`, one call per item).
        let slice_body = self.with_plain_calls(true, || {
            self.stream_level(path, 0, &view, &mut TokenStream::new(), false)
        });
        if self.dual {
            // The same steps again from the plain rules, for an input that is
            // all there (`step_call`); their calls are already in `body`.
            self.plain_steps.set(true);
            self.streaming.set(false);
            let plain_end = self.step_name(format!("__stream_{}_end", rule.name));
            helpers.extend(end_def(&plain_end, &self.ws()));
            let _ = self.stream_level(path, 0, &view, &mut helpers, false);
            self.streaming.set(true);
            self.plain_steps.set(false);
        }
        let finish = |plain: bool| {
            let call = step_call(&view, &end_fn, &[], plain);
            let keep = step_keep(&kind, &view, &end_fn, &[]);
            quote! { #call, #keep }
        };
        let (finish, slice_finish) = (finish(self.dual), finish(self.dual));
        let slice_entry = self.stream_slice_fn(rule, &slice_body, &slice_finish, (&params, &names));
        let each_fn = if path.each {
            self.stream_each_fn(
                rule,
                path,
                &view,
                &kind,
                (&finish, &slice_finish),
                (&params, &names),
            )
        } else {
            quote! {}
        };
        quote_spanned! {span=>
            #helpers
            #each_fn

            /// Parses what `reader` yields, one item of the rule's `fold` at
            /// a time, in a buffer of at most `reading.max_frame` bytes past
            /// the start of each item, plus a block (ADR 27 §3). The answer
            /// is the parse of the whole input in memory; errors are at their
            /// offset in the stream. `context` is the parse's: its user state
            /// and its recoveries are there when the call returns.
            #[allow(dead_code, non_snake_case, unused_mut, unused_variables, clippy::let_unit_value)]
            #vis fn #reader_fn<__WgS: ::std::fmt::Debug + Clone #state_bound>(
                reader: impl ::std::io::Read,
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                reading: ::winnow_grammar::rt::Reading,
                #(#params),*
            ) -> ::core::result::Result<#ret, ::winnow_grammar::ParseError> {
                let mut __stream = ::winnow_grammar::rt::stream::Stream::<#kind, _, __WgS, #reach>::new(
                    reader, context, reading, #k,
                );
                let __value = #body;
                __stream.finish(#finish)?;
                ::core::result::Result::Ok(__value)
            }

            #slice_entry
        }
    }

    /// `parse_<rule>_slice`: the stream rule over an input all in memory,
    /// its passes those of the parse in memory (`rt::stream::slice_passes`);
    /// and in a text grammar `parse_<rule>_str`, the same over a `&str`,
    /// which is not checked as UTF-8 again. Both call one function, which
    /// takes whether the input is checked.
    fn stream_slice_fn(
        &self,
        rule: &Rule,
        slice_body: &TokenStream,
        slice_finish: &TokenStream,
        (params, names): (&[TokenStream], &[&syn::Ident]),
    ) -> TokenStream {
        let span = Span::mixed_site();
        let state_bound = self.state_bound();
        let vis = &rule.vis;
        let ret = &rule.return_type;
        let kind = self.stream_kind();
        let (k, reach) = (self.edge_k, self.reach);
        let slice_fn = format_ident!("parse_{}_slice", rule.name, span = span);
        let whole_fn = format_ident!("__wg_{}_whole", rule.name, span = span);
        let doc = slice_doc(&format!("parse_{}_reader", rule.name));
        let str_entry = self.str_entry(
            rule,
            &whole_fn,
            &quote! { ::core::result::Result<#ret, ::winnow_grammar::ParseError> },
            (params, names),
            &[],
        );
        quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_mut, unused_variables, clippy::let_unit_value)]
            fn #whole_fn<__WgS: ::std::fmt::Debug + Clone #state_bound>(
                input: &[u8],
                checked: bool,
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                #(#params),*
            ) -> ::core::result::Result<#ret, ::winnow_grammar::ParseError> {
                let mut __run = |context: &mut ::winnow_grammar::ParseContext<__WgS>, #(#params),*|
                    -> ::core::result::Result<#ret, ::winnow_grammar::ParseError>
                {
                    let mut __stream = ::winnow_grammar::rt::stream::Stream::<#kind, _, __WgS, #reach, true>::whole(
                        input, checked, context, #k,
                    );
                    let __value = #slice_body;
                    __stream.finish(#slice_finish)?;
                    ::core::result::Result::Ok(__value)
                };
                ::winnow_grammar::rt::stream::slice_passes(context, |context, _| {
                    __run(context, #(::core::clone::Clone::clone(&#names)),*)
                })
            }

            #[doc = #doc]
            #[allow(dead_code, non_snake_case, clippy::let_unit_value)]
            #vis fn #slice_fn<__WgS: ::std::fmt::Debug + Clone #state_bound>(
                input: &[u8],
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                #(#params),*
            ) -> ::core::result::Result<#ret, ::winnow_grammar::ParseError> {
                #whole_fn(input, false, context, #(#names),*)
            }

            #str_entry
        }
    }

    /// `parse_<rule>_str` / `parse_<rule>_str_each` in a text grammar: the
    /// slice entry over a `&str`, whose bytes the stream does not check as
    /// UTF-8 again. Nothing in a byte grammar.
    fn str_entry(
        &self,
        rule: &Rule,
        whole_fn: &syn::Ident,
        ret: &TokenStream,
        (params, names): (&[TokenStream], &[&syn::Ident]),
        each: &[TokenStream],
    ) -> TokenStream {
        if self.is_bytes() {
            return quote! {};
        }
        let span = Span::mixed_site();
        let state_bound = self.state_bound();
        let vis = &rule.vis;
        let (str_fn, slice_fn, generics) = if each.is_empty() {
            (
                format_ident!("parse_{}_str", rule.name, span = span),
                format!("parse_{}_slice", rule.name),
                quote! {},
            )
        } else {
            (
                format_ident!("parse_{}_str_each", rule.name, span = span),
                format!("parse_{}_slice_each", rule.name),
                quote! { , __WgB },
            )
        };
        let doc = format!(
            " [`{slice_fn}`] over a `&str`: the same parse, without checking the input as\n \
             UTF-8 again."
        );
        let each_arg = if each.is_empty() {
            quote! {}
        } else {
            quote! { each }
        };
        quote_spanned! {span=>
            #[doc = #doc]
            #[allow(dead_code, non_snake_case, clippy::let_unit_value)]
            #vis fn #str_fn<__WgS: ::std::fmt::Debug + Clone #state_bound #generics>(
                input: &str,
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                #(#params,)*
                #(#each)*
            ) -> #ret {
                #whole_fn(input.as_bytes(), true, context, #(#names,)* #each_arg)
            }
        }
    }

    /// `parse_<rule>_reader_each` of a stream rule whose fold has `each`:
    /// the reader function, showing every item to a callback.
    fn stream_each_fn(
        &self,
        rule: &Rule,
        path: &StreamPath,
        view: &TokenStream,
        kind: &TokenStream,
        (finish, slice_finish): (&TokenStream, &TokenStream),
        (params, names): (&[TokenStream], &[&syn::Ident]),
    ) -> TokenStream {
        let span = Span::mixed_site();
        let state_bound = self.state_bound();
        let vis = &rule.vis;
        let ret = &rule.return_type;
        let k = self.edge_k;
        let reach = self.reach;
        let each_fn = format_ident!("parse_{}_reader_each", rule.name, span = span);
        let slice_each_fn = format_ident!("parse_{}_slice_each", rule.name, span = span);
        let slice_doc = slice_doc(&format!("parse_{}_reader_each", rule.name));
        let body = self.with_plain_calls(false, || {
            self.stream_level(path, 0, view, &mut TokenStream::new(), true)
        });
        let slice_body = self.with_plain_calls(true, || {
            self.stream_level(path, 0, view, &mut TokenStream::new(), true)
        });
        let item = self.stream_item_type(path);
        let emitted = emitted_type(&item);
        let whole_each_fn = format_ident!("__wg_{}_whole_each", rule.name, span = span);
        let str_each = self.str_entry(
            rule,
            &whole_each_fn,
            &quote! { ::core::result::Result<::core::ops::ControlFlow<__WgB, #ret>, ::winnow_grammar::ParseError> },
            (params, names),
            &[quote! { each: impl for<'a> FnMut(&#item) -> ::core::ops::ControlFlow<__WgB>, }],
        );
        quote_spanned! {span=>
            /// [`#reader_fn`], showing `each` every item of the fold as
            /// it is taken, before the fold's step takes it. `each`
            /// returning `Break(b)` ends the parse there - nothing after
            /// that item is read - with `Ok(Break(b))`; a parse that
            /// reads to the end is `Ok(Continue(value))`. Items are seen
            /// once each, in order; the grammar's fold runs as it always
            /// does (ADR 27 §3).
            #[allow(dead_code, non_snake_case, unused_mut, unused_variables, clippy::let_unit_value)]
            #vis fn #each_fn<__WgS: ::std::fmt::Debug + Clone #state_bound, __WgB>(
                reader: impl ::std::io::Read,
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                reading: ::winnow_grammar::rt::Reading,
                #(#params,)*
                mut each: impl for<'a> FnMut(&#item) -> ::core::ops::ControlFlow<__WgB>,
            ) -> ::core::result::Result<::core::ops::ControlFlow<__WgB, #ret>, ::winnow_grammar::ParseError> {
                let mut __broken: ::core::option::Option<__WgB> = ::core::option::Option::None;
                let __again = false;
                let mut __stream = ::winnow_grammar::rt::stream::Stream::<#kind, _, __WgS, #reach>::new(
                    reader, context, reading, #k,
                );
                // `emit` in an action reaches `each` from here on; dropping
                // `__stream` closes the sink again (ADR 28).
                __stream.context().emits.open::<#emitted>();
                let __value = #body;
                __stream.finish(#finish)?;
                ::core::result::Result::Ok(::core::ops::ControlFlow::Continue(__value))
            }

            #[allow(dead_code, non_snake_case, unused_mut, unused_variables, clippy::let_unit_value)]
            fn #whole_each_fn<__WgS: ::std::fmt::Debug + Clone #state_bound, __WgB>(
                input: &[u8],
                checked: bool,
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                #(#params,)*
                mut each: impl for<'a> FnMut(&#item) -> ::core::ops::ControlFlow<__WgB>,
            ) -> ::core::result::Result<::core::ops::ControlFlow<__WgB, #ret>, ::winnow_grammar::ParseError> {
                let mut __run = |context: &mut ::winnow_grammar::ParseContext<__WgS>, __again: bool, #(#params),*|
                    -> ::core::result::Result<::core::ops::ControlFlow<__WgB, #ret>, ::winnow_grammar::ParseError>
                {
                    let mut __broken: ::core::option::Option<__WgB> = ::core::option::Option::None;
                    let mut __stream = ::winnow_grammar::rt::stream::Stream::<#kind, _, __WgS, #reach, true>::whole(
                        input, checked, context, #k,
                    );
                    __stream.context().emits.open::<#emitted>();
                    let __value = #slice_body;
                    __stream.finish(#slice_finish)?;
                    ::core::result::Result::Ok(::core::ops::ControlFlow::Continue(__value))
                };
                ::winnow_grammar::rt::stream::slice_passes(context, |context, __again| {
                    __run(context, __again, #(::core::clone::Clone::clone(&#names)),*)
                })
            }

            #[doc = #slice_doc]
            #[allow(dead_code, non_snake_case, clippy::let_unit_value)]
            #vis fn #slice_each_fn<__WgS: ::std::fmt::Debug + Clone #state_bound, __WgB>(
                input: &[u8],
                context: &mut ::winnow_grammar::ParseContext<__WgS>,
                #(#params,)*
                each: impl for<'a> FnMut(&#item) -> ::core::ops::ControlFlow<__WgB>,
            ) -> ::core::result::Result<::core::ops::ControlFlow<__WgB, #ret>, ::winnow_grammar::ParseError> {
                #whole_each_fn(input, false, context, #(#names,)* each)
            }

            #str_each
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
        each: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let level: &Level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let v = &rule.variants[0];
        let at = level.at;
        let (prefix, prefix_cut) = self.stream_prefix(path, j, view, helpers);
        // `_reader_each`: what a step emitted goes to `each` once it matched.
        let deliver = if each {
            let item = self.stream_item_type(path);
            deliver_emits(&emitted_type(&item))
        } else {
            TokenStream::new()
        };
        let after_prefix = deliver.clone();

        // --- The element at `at`: the next rule's level, or the fold.
        let (middle, middle_value) = match path.levels.get(j + 1) {
            Some(next) => {
                let inner = self.stream_level(path, j + 1, view, helpers, each);
                let ty = self.rule(&next.rule).return_type.clone();
                (quote_spanned! {span=> let __path: #ty = #inner; }, Some(ty))
            }
            None => (self.stream_fold(path, j, view, helpers, each), None),
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
        let suffix = self.stream_suffix(path, j, view, helpers, binding.as_ref(), path_param);
        let after_suffix = if suffix.is_empty() {
            TokenStream::new()
        } else {
            deliver.clone()
        };

        // --- The action, between steps, with the context.
        let ret = &rule.return_type;
        let action = &v.action;
        let action = if v.is_explicit {
            quote! { { #action } }
        } else {
            quote! { #action }
        };
        // A cut before `at` is one scope from the prefix through the suffix,
        // as the rule's sequence is in memory: what it set aside competes
        // again once the suffix matched (`Stream::cut_end`).
        let (scope_begin, scope_end) = if prefix_cut {
            let depth = format_ident!("__wg_stream_cut", span = span);
            (
                quote_spanned! {span=> let #depth = __stream.cut_depth(); },
                quote_spanned! {span=> __stream.cut_end(#depth); },
            )
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        // The action names `_state` as an action in memory does: the binding
        // is the user's, not the macro's.
        let state = proc_macro2::Ident::new("_state", Span::call_site());
        quote_spanned! {span=>
            {
                #scope_begin
                #prefix
                #after_prefix
                #middle
                #bind_path
                #suffix
                #after_suffix
                #scope_end
                let #state = __stream.context();
                let __r: ::winnow::Result<#ret, ::winnow::error::ErrMode<::winnow_grammar::ParseError>> =
                    (|| ::core::result::Result::Ok(#action))();
                let __level = match __r {
                    ::core::result::Result::Ok(v) => v,
                    ::core::result::Result::Err(e) => {
                        return ::core::result::Result::Err(::winnow_grammar::rt::stream::action_error(e))
                    }
                };
                #deliver
                __level
            }
        }
    }

    /// The prefix step of level `j`: the entry point's skip at the top, the
    /// rule's own at its start, its elements before `at`, and the skip
    /// before the element at `at`. Its function goes to `helpers`; returned
    /// are the statement that runs it and whether it has a cut.
    fn stream_prefix(
        &self,
        path: &StreamPath,
        j: usize,
        view: &TokenStream,
        helpers: &mut TokenStream,
    ) -> (TokenStream, bool) {
        let ws = self.ws();
        let span = Span::mixed_site();
        let level: &Level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let v = &rule.variants[0];
        let at = level.at;
        let lexical = rule.is_lexical;
        let input = &self.input_ident;
        let skip = quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(#ws, #input)?; };
        let params = value_params(rule);
        let prefix_fn = self.step_name(format!("__stream_{}_{}_prefix", path.levels[0].rule, j));
        let mut steps = TokenStream::new();
        if j == 0 {
            steps.extend(skip.clone());
        }
        if !lexical {
            steps.extend(skip);
        }
        // A cut here holds into the suffix, so its scope is not closed here:
        // the driver closes it after the suffix (`scope_begin`/`scope_end`).
        steps.extend(self.generate_sequence_steps_unscoped(&v.pattern[..at], false, lexical));
        // The skip before the element at `at`: under the prefix's cut, if
        // it has one, as in memory.
        let prefix_cut = v.pattern[..at]
            .iter()
            .any(|p| matches!(p, ModelPattern::Cut(_)));
        if !lexical && at > 0 {
            steps.extend(self.skip_stmt(input, prefix_cut));
        }
        let next_params = match path.levels.get(j + 1) {
            Some(next) => value_params(self.rule(&next.rule)),
            None => Vec::new(),
        };
        let next_args: Vec<TokenStream> = match &v.pattern[at] {
            ModelPattern::RuleCall { args, .. } if j + 1 < path.levels.len() => args
                .iter()
                .map(|a| self.generate_argument_expr(a, lexical))
                .collect(),
            _ => Vec::new(),
        };
        let (c_names, c_types) = (names(&level.carried), types(&level.carried));
        let a_types = types(&next_params);
        let p_typed = typed(&params);
        let (input_ty, rt_error, state_bound) =
            (self.input_ty(), self.rt_error(), self.state_bound());
        helpers.extend(quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_variables, unused_mut, clippy::let_unit_value)]
            fn #prefix_fn<'a, __WgC: ::winnow_grammar::Ctx<State: ::std::fmt::Debug + Clone #state_bound>, __WgEr: #rt_error>(
                #input: &mut #input_ty,
                #(#p_typed),*
            ) -> ::winnow::Result<((#(#c_types,)*), (#(#a_types,)*)), ::winnow::error::ErrMode<__WgEr>> {
                use ::winnow::Parser;
                use ::winnow::prelude::*;
                #steps
                ::core::result::Result::Ok(((#(#c_names,)*), (#(#next_args,)*)))
            }
        });
        let a_names = names(&next_params);
        let rules = path_rules(path, j);
        let prefix_call = step_call(view, &prefix_fn, &clones(&params), self.plain_calls.get());
        let prefix_keep = step_keep(&self.stream_kind(), view, &prefix_fn, &clones(&params));
        let prefix = quote_spanned! {span=>
            let ((#(#c_names,)*), (#(#a_names,)*)) = __stream.chunk(&[#(#rules),*], #prefix_call, #prefix_keep)?;
        };
        (prefix, prefix_cut)
    }

    /// The suffix step of level `j`: its elements after `at`, after the
    /// skip before the first. It takes the rule's parameters, what the
    /// prefix carried and the value at `at` (when that is bound and is the
    /// next level's); its function goes to `helpers`, the statement that
    /// runs it is returned.
    fn stream_suffix(
        &self,
        path: &StreamPath,
        j: usize,
        view: &TokenStream,
        helpers: &mut TokenStream,
        binding: Option<&syn::Ident>,
        path_param: Option<syn::Type>,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let level: &Level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let v = &rule.variants[0];
        let at = level.at;
        let lexical = rule.is_lexical;
        let input = &self.input_ident;
        let suffix_fn = self.step_name(format!("__stream_{}_{}_suffix", path.levels[0].rule, j));
        let suffix_pats = &v.pattern[at + 1..];
        let cut = v.pattern[..=at]
            .iter()
            .any(|p| matches!(p, ModelPattern::Cut(_)));
        let mut steps = TokenStream::new();
        if !lexical && !suffix_pats.is_empty() {
            steps.extend(self.skip_stmt(input, cut));
        }
        steps.extend(self.generate_sequence_steps(suffix_pats, cut, lexical));
        let returned = &level.returned;
        let (r_names, r_types) = (names(returned), types(returned));
        let mut s_in = value_params(rule);
        s_in.extend(level.carried.iter().cloned());
        if let (Some(b), Some(ty)) = (binding, path_param) {
            s_in.push((b.clone(), ty));
        }
        let s_typed = typed(&s_in);
        let (input_ty, rt_error, state_bound) =
            (self.input_ty(), self.rt_error(), self.state_bound());
        helpers.extend(quote_spanned! {span=>
            #[allow(dead_code, non_snake_case, unused_variables, unused_mut, clippy::let_unit_value)]
            fn #suffix_fn<'a, __WgC: ::winnow_grammar::Ctx<State: ::std::fmt::Debug + Clone #state_bound>, __WgEr: #rt_error>(
                #input: &mut #input_ty,
                #(#s_typed),*
            ) -> ::winnow::Result<(#(#r_types,)*), ::winnow::error::ErrMode<__WgEr>> {
                use ::winnow::Parser;
                use ::winnow::prelude::*;
                #steps
                ::core::result::Result::Ok((#(#r_names,)*))
            }
        });
        let rules = path_rules(path, j);
        let suffix_call = step_call(view, &suffix_fn, &clones(&s_in), self.plain_calls.get());
        let suffix_keep = step_keep(&self.stream_kind(), view, &suffix_fn, &clones(&s_in));
        quote_spanned! {span=>
            let (#(#r_names,)*) = __stream.chunk(&[#(#rules),*], #suffix_call, #suffix_keep)?;
        }
    }

    /// Emits the two helpers of a streamed fold: the item parser (with the
    /// separator before it, past the first item) and the identity function
    /// that makes the step take items of any buffer's lifetime. Returns
    /// whether the step changes its accumulator in place.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of one fold, taken apart by the caller"
    )]
    fn stream_fold_fns(
        &self,
        stream_rule: &str,
        item_rule: &str,
        pattern: &ModelPattern,
        sep: Option<&ModelPattern>,
        step: &syn::Expr,
        lexical: bool,
        helpers: &mut TokenStream,
    ) -> bool {
        let ws = self.ws();
        let span = Span::mixed_site();
        let item_rule = self.rule(item_rule);
        let input = &self.input_ident;
        let input_ty = self.input_ty();
        let rt_error = self.rt_error();
        let state_bound = self.state_bound();
        let item_ret = &item_rule.return_type;
        let item_inner = self.inner_name(&item_rule.name.to_string(), self.edge_copy());
        let item_fn = self.step_name(format!("__stream_{}_item", stream_rule));
        let step_fn = self.step_name(format!("__stream_{}_step", stream_rule));
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
            quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(#ws, #input)?; }
        };
        // A separator comes before every item but the first, and is taken
        // only with the item after it, as in memory.
        let sep_param = if sep.is_some() {
            quote_spanned! {span=> __sep: bool, }
        } else {
            quote! {}
        };
        let sep_parse = match sep {
            Some(sep) => {
                let sep_parser = self.generate_parser_expr(sep, lexical, true);
                quote_spanned! {span=>
                    if __sep {
                        #skip
                        let mut __s = #sep_parser;
                        ::winnow::Parser::parse_next(&mut __s, #input)?;
                    }
                }
            }
            None => quote! {},
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
            step,
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
            fn #item_fn<'a, __WgC: ::winnow_grammar::Ctx<State: ::std::fmt::Debug + Clone #state_bound>, __WgEr: #rt_error>(
                #input: &mut #input_ty,
                #sep_param
                #(#a_names: #a_types),*
            ) -> ::winnow::Result<#item_ret, ::winnow::error::ErrMode<__WgEr>> {
                use ::winnow::Parser;
                #sep_parse
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
        in_place
    }

    /// The fold of the last rule of the path: `__path` is its value.
    fn stream_fold(
        &self,
        path: &StreamPath,
        j: usize,
        view: &TokenStream,
        helpers: &mut TokenStream,
        each: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let level = &path.levels[j];
        let rule = self.rule(&level.rule);
        let stream_rule = &path.levels[0].rule;
        let lexical = rule.is_lexical;
        let rules: Vec<&str> = path.levels[..=j].iter().map(|l| l.rule.as_str()).collect();
        let ModelPattern::Fold {
            pattern,
            init,
            step,
            sep,
            min,
            max,
            computed,
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
        let item_rule = rule_path.segments.last().unwrap().ident.to_string();
        let item_fn = self.step_name(format!("__stream_{}_item", stream_rule));
        let step_fn = self.step_name(format!("__stream_{}_step", stream_rule));
        let held_view = self.held_view();
        let in_place = self.stream_fold_fns(
            stream_rule,
            &item_rule,
            pattern,
            sep.as_deref(),
            step,
            lexical,
            helpers,
        );
        let sep_arg = if sep.is_some() {
            quote_spanned! {span=> __item > 1, }
        } else {
            quote! {}
        };
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
        let emitted = each.then(|| emitted_type(&self.stream_item_type(path)));
        let (tap, stopped) = each_tap(emitted.as_ref());
        let (bounds, at_max, short) = fold_bounds(computed.as_deref(), *min, *max);
        let item_keep = item_keep(
            &self.stream_kind(),
            view,
            &item_fn,
            &sep_arg,
            &args,
            &at_max_check(computed.as_deref(), *max, quote! { () }),
        );
        // The step is always taken into the reader's loop: called, it was a
        // frame of its own around the item rule every item, once that rule's
        // tracked copy had one caller and was taken into it. Its fast pass
        // runs with `EdgeEmpty`, so that what the tracked copy reaches is
        // compiled apart from the plain copy (ADR 27 §3, *Per-mode copies*).
        let plain_item = self.plain_item_call(&item_fn, &sep_arg, &args);
        quote_spanned! {span=>
            #bounds
            let mut __acc = ::core::option::Option::Some((#init)());
            let mut __step = #step_fn(&__acc, #step);
            let mut __n = 0usize;
            while __stream.items(&[#(#rules),*], &mut __n, #[inline(always)] |input: &mut #held_view, pass: &::winnow_grammar::rt::stream::Pass, __item: usize| {
                #at_max
                let __v = #plain_item if pass.diagnose() {
                    #item_fn::<_, ::winnow_grammar::ParseError>(input, #sep_arg #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                } else {
                    #item_fn::<_, ::winnow_grammar::rt::stream::EdgeEmpty>(input, #sep_arg #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                };
                if ::winnow_grammar::rt::stream::commit(input, pass) {
                    #tap
                    #apply
                    ::core::result::Result::Ok(true)
                } else {
                    ::core::result::Result::Ok(false)
                }
            }, #item_keep)? {}
            #stopped
            #short
            let __path = __acc.take().expect("the accumulator is there");
        }
    }

    /// With the input all there, the item function from the plain rules, as
    /// for a step (`step_call`): the start of an `if … else` before the
    /// tracked calls, or nothing where the grammar has one copy.
    fn plain_item_call(
        &self,
        item_fn: &syn::Ident,
        sep_arg: &TokenStream,
        args: &[TokenStream],
    ) -> TokenStream {
        if !self.plain_items.get() {
            return quote! {};
        }
        let span = Span::mixed_site();
        let fp = format_ident!("{}_plain", item_fn, span = span);
        quote_spanned! {span=>
            if pass.last() {
                if pass.diagnose() {
                    #fp::<_, ::winnow_grammar::ParseError>(input, #sep_arg #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                } else {
                    #fp::<_, ::winnow::error::EmptyError>(input, #sep_arg #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)?
                }
            } else
        }
    }

    /// The type of the items of a path's fold: its item rule's.
    fn stream_item_type(&self, path: &StreamPath) -> syn::Type {
        let level = path.levels.last().expect("a path has a level");
        let ModelPattern::Fold { pattern, .. } =
            &self.rule(&level.rule).variants[0].pattern[level.at]
        else {
            unreachable!("the model checked that the path ends in a fold")
        };
        let ModelPattern::RuleCall { rule_path, .. } = &**pattern else {
            unreachable!("the model checked that a streamed fold's item is a rule call")
        };
        self.rule(&rule_path.segments.last().unwrap().ident.to_string())
            .return_type
            .clone()
    }

    /// The stream's `rt::stream::Kind`.
    fn stream_kind(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::rt::stream::Bytes }
        } else {
            quote! { ::winnow_grammar::rt::stream::Text }
        }
    }

    /// The input of the item loop: it holds the context for the window's
    /// length (`rt::stream::Held`), so that the loop keeps its fields at
    /// fixed places instead of behind the pointer it is lent through.
    fn held_view(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::ParseBytes<'_, __WgS> }
        } else {
            quote! { ::winnow_grammar::ParseInput<'_, __WgS> }
        }
    }

    /// The input of one step.
    fn stream_view(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::ParseBytesOf<'_, &mut ::winnow_grammar::ParseContext<__WgS>> }
        } else {
            quote! { ::winnow_grammar::ParseInputOf<'_, &mut ::winnow_grammar::ParseContext<__WgS>> }
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

/// A value parameter or carried value of a step function: name and type.
type Value = (syn::Ident, syn::Type);

/// The parameters of a rule on a stream path, which are all values.
fn value_params(rule: &Rule) -> Vec<Value> {
    rule.params
        .iter()
        .map(|p| (p.name.clone(), p.ty.clone().expect("value parameter")))
        .collect()
}

/// `name: Type` for each value, as parameters.
fn typed(vals: &[Value]) -> Vec<TokenStream> {
    vals.iter().map(|(n, t)| quote! { #n: #t }).collect()
}

/// The names of the values.
fn names(vals: &[Value]) -> Vec<syn::Ident> {
    vals.iter().map(|(n, _)| n.clone()).collect()
}

/// The types of the values.
fn types(vals: &[Value]) -> Vec<syn::Type> {
    vals.iter().map(|(_, t)| t.clone()).collect()
}

/// A clone of each value, as arguments.
fn clones(vals: &[Value]) -> Vec<TokenStream> {
    vals.iter()
        .map(|(n, _)| quote! { ::core::clone::Clone::clone(&#n) })
        .collect()
}

/// The rules of the path down to level `j`, for the stream's error context.
fn path_rules(path: &StreamPath, j: usize) -> Vec<&str> {
    path.levels[..=j].iter().map(|l| l.rule.as_str()).collect()
}

/// The documentation of a slice entry: `reader`, over an input all in
/// memory.
fn slice_doc(reader: &str) -> String {
    format!(
        " [`{reader}`] over an input that is all in memory: every step knows it has the\n \
         rest of the input and runs the plain parsers. Its passes are those of the parse\n \
         in memory (ADR 17): a fast pass and, only if that fails, a diagnosing one."
    )
}

/// The closure `Stream::chunk` runs a step function `f` with: in the
/// diagnosing pass with `ParseError`, otherwise with `EmptyError`.
fn step_call(view: &TokenStream, f: &syn::Ident, args: &[TokenStream], plain: bool) -> TokenStream {
    let span = Span::mixed_site();
    // With the input all there, the step function from the plain rules:
    // the end of the window cannot be what a parse depended on.
    let plain_call = if plain {
        let fp = format_ident!("{}_plain", f, span = span);
        quote_spanned! {span=>
            if pass.last() {
                return if pass.diagnose() {
                    #fp::<_, ::winnow_grammar::ParseError>(input, #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)
                } else {
                    #fp::<_, ::winnow::error::EmptyError>(input, #(#args),*)
                        .map_err(::winnow_grammar::rt::stream::Fail::from)
                };
            }
        }
    } else {
        quote! {}
    };
    quote_spanned! {span=>
        |input: &mut #view, pass: &::winnow_grammar::rt::stream::Pass, _: usize| {
            #plain_call
            if pass.diagnose() {
                #f::<_, ::winnow_grammar::ParseError>(input, #(#args),*)
                    .map_err(::winnow_grammar::rt::stream::Fail::from)
            } else {
                #f::<_, ::winnow_grammar::rt::stream::EdgeEmpty>(input, #(#args),*)
                    .map_err(::winnow_grammar::rt::stream::Fail::from)
            }
        }
    }
}

/// The item of a streamed fold as the diagnosing pass parses it again
/// (`rt::stream::Replayer`): its arguments as they are when the fold ends,
/// nothing taken.
fn item_keep(
    kind: &TokenStream,
    view: &TokenStream,
    item_fn: &syn::Ident,
    sep_arg: &TokenStream,
    args: &[TokenStream],
    at_max: &TokenStream,
) -> TokenStream {
    let span = Span::mixed_site();
    let names: Vec<syn::Ident> = (0..args.len())
        .map(|i| format_ident!("__wg_keep{}", i, span = span))
        .collect();
    quote_spanned! {span=>
        || {
            #(let #names = #args;)*
            ::winnow_grammar::rt::stream::replayer::<#kind, __WgS, _, _>(
                move |input: &mut #view, _: &::winnow_grammar::rt::stream::Pass, __item: usize| {
                    #at_max
                    #item_fn::<_, ::winnow_grammar::ParseError>(
                        input, #sep_arg #(::core::clone::Clone::clone(&#names)),*
                    )
                    .map(|_| ())
                    .map_err(::winnow_grammar::rt::stream::Fail::from)
                },
            )
        }
    }
}

/// What [`step_call`] runs, as the diagnosing pass runs it again
/// (`rt::stream::Replayer`): the arguments taken once, when the stream asks,
/// and cloned for every replay.
fn step_keep(
    kind: &TokenStream,
    view: &TokenStream,
    f: &syn::Ident,
    args: &[TokenStream],
) -> TokenStream {
    let span = Span::mixed_site();
    let names: Vec<syn::Ident> = (0..args.len())
        .map(|i| format_ident!("__wg_keep{}", i, span = span))
        .collect();
    quote_spanned! {span=>
        || {
            #(let #names = #args;)*
            ::winnow_grammar::rt::stream::replayer::<#kind, __WgS, _, _>(
                move |input: &mut #view, _: &::winnow_grammar::rt::stream::Pass, _: usize| {
                    #f::<_, ::winnow_grammar::ParseError>(
                        input, #(::core::clone::Clone::clone(&#names)),*
                    )
                    .map_err(::winnow_grammar::rt::stream::Fail::from)
                },
            )
        }
    }
}

/// The bounds of a streamed fold, as the code before its loop (computed
/// sides, ADR 26 D3), the check that ends it at `max`, and the check after
/// it that fails short of `min`.
fn fold_bounds(
    computed: Option<&FoldBounds>,
    min: usize,
    max: Option<usize>,
) -> (TokenStream, TokenStream, TokenStream) {
    let span = Span::mixed_site();
    // Computed sides (ADR 26 D3) are evaluated once, before the first
    // item; a `min` above the `max` fails as too few items would.
    let bounds = match computed {
        Some(c) => {
            let lo = match &c.min {
                Some(e) => quote_spanned! {span=> #e },
                None => quote_spanned! {span=> #min },
            };
            let hi = match (&c.max, max) {
                (Some(e), _) => quote_spanned! {span=> ::core::option::Option::Some(#e) },
                (None, Some(m)) => quote_spanned! {span=> ::core::option::Option::Some(#m) },
                (None, None) => quote_spanned! {span=> ::core::option::Option::None::<usize> },
            };
            quote_spanned! {span=>
                let (__lo, __hi) = match ::winnow_grammar::rt::bounds(#lo, #hi) {
                    ::core::option::Option::Some(__b) => __b,
                    ::core::option::Option::None => {
                        return ::core::result::Result::Err(__stream.short());
                    }
                };
            }
        }
        None => quote! {},
    };
    // `max = N`: the fold takes no item past the N-th.
    let at_max = at_max_check(computed, max, quote! { false });
    // `min = N`: fewer items fail the parse with what the next one
    // expected, as in memory.
    let short = if computed.is_some() {
        quote_spanned! {span=>
            if __n < __lo {
                return ::core::result::Result::Err(__stream.short());
            }
        }
    } else if min > 0 {
        quote_spanned! {span=>
            if __n < #min {
                return ::core::result::Result::Err(__stream.short());
            }
        }
    } else {
        quote! {}
    };
    (bounds, at_max, short)
}

/// The check that ends a streamed fold at `max = N`: past the N-th item the
/// item step returns `Ok(value)` before parsing.
fn at_max_check(
    computed: Option<&FoldBounds>,
    max: Option<usize>,
    value: TokenStream,
) -> TokenStream {
    let span = Span::mixed_site();
    match (computed, max) {
        (Some(_), _) => quote_spanned! {span=>
            if let ::core::option::Option::Some(__m) = __hi {
                if __item > __m {
                    return ::core::result::Result::Ok(#value);
                }
            }
        },
        (None, Some(m)) => quote_spanned! {span=>
            if __item > #m {
                return ::core::result::Result::Ok(#value);
            }
        },
        (None, None) => quote! {},
    }
}

/// For `_reader_each`: the check that hands each item to the caller before
/// the step takes it (and stops the parse on `Break`), and the check after
/// the loop that returns that `Break`. Empty otherwise.
fn each_tap(emitted: Option<&syn::Type>) -> (TokenStream, TokenStream) {
    let span = Span::mixed_site();
    // `_reader_each`: the caller sees the item before the step takes it,
    // and may stop the parse there - and before the item, what its parse
    // emitted (ADR 28).
    if let Some(emitted) = emitted {
        (
            quote_spanned! {span=>
                for __e in ::winnow_grammar::Ctx::cx_mut(&mut input.state).emits.drain::<#emitted>() {
                    if let ::core::ops::ControlFlow::Break(__b) = ::winnow_grammar::rt::stream::see(__again, &mut each, &__e) {
                        __broken = ::core::option::Option::Some(__b);
                        return ::core::result::Result::Err(::winnow_grammar::rt::stream::Fail::Stop);
                    }
                }
                if let ::core::ops::ControlFlow::Break(__b) = ::winnow_grammar::rt::stream::see(__again, &mut each, &__v) {
                    __broken = ::core::option::Option::Some(__b);
                    return ::core::result::Result::Err(::winnow_grammar::rt::stream::Fail::Stop);
                }
            },
            quote_spanned! {span=>
                if let ::core::option::Option::Some(__b) = __broken.take() {
                    return ::core::result::Result::Ok(::core::ops::ControlFlow::Break(__b));
                }
            },
        )
    } else {
        (quote! {}, quote! {})
    }
}

/// For `_reader_each`: hands what the step just taken emitted to `each`,
/// oldest first, and returns `Break` from the parse where `each` asks for
/// it - as a `Break` on an item does (ADR 28).
fn deliver_emits(emitted: &syn::Type) -> TokenStream {
    let span = Span::mixed_site();
    quote_spanned! {span=>
        for __e in __stream.context().emits.drain::<#emitted>() {
            if let ::core::ops::ControlFlow::Break(__b) = ::winnow_grammar::rt::stream::see(__again, &mut each, &__e) {
                return ::core::result::Result::Ok(::core::ops::ControlFlow::Break(__b));
            }
        }
    }
}

/// The type an action emits for items of type `item`: the same with
/// `'static` for every lifetime, named or elided, since an emitted value is
/// held past the input it was parsed from and is told apart by its
/// `TypeId`. `each` takes `&Item`, which such a value is where `Item` is
/// covariant in its lifetimes, as a type built of references, owned data
/// and other such types is.
fn emitted_type(item: &syn::Type) -> syn::Type {
    use syn::visit_mut::VisitMut;
    struct Static;
    impl VisitMut for Static {
        fn visit_lifetime_mut(&mut self, l: &mut syn::Lifetime) {
            *l = syn::Lifetime::new("'static", l.span());
        }
        fn visit_type_reference_mut(&mut self, r: &mut syn::TypeReference) {
            if r.lifetime.is_none() {
                r.lifetime = Some(syn::Lifetime::new("'static", Span::call_site()));
            }
            syn::visit_mut::visit_type_reference_mut(self, r);
        }
    }
    let mut ty = item.clone();
    Static.visit_type_mut(&mut ty);
    ty
}
