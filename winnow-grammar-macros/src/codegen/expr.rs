use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use std::collections::HashMap;
use winnow_grammar_model::model::{Argument, ModelPattern};

pub(crate) fn set_binding(pattern: &mut ModelPattern, new_binding: Option<syn::Ident>) {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => *binding = new_binding,
        ModelPattern::Group { binding, .. } => *binding = new_binding,
        ModelPattern::Lit { binding, .. } => *binding = new_binding,
        ModelPattern::Recover { binding, .. } => *binding = new_binding,
        ModelPattern::Until { binding, .. } => *binding = new_binding,
        ModelPattern::Count { binding, .. } | ModelPattern::Fold { binding, .. } => {
            *binding = new_binding
        }
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::Bounded { pattern: inner, .. }
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::LexicalScope(inner, _)
        | ModelPattern::SpacedScope(inner, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _) => set_binding(inner, new_binding),
        ModelPattern::Parenthesized(inner, _)
        | ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _) => {
            // Only with exactly one element is it unambiguous what the
            // binding refers to.
            if let [single] = inner.as_mut_slice() {
                set_binding(single, new_binding);
            }
        }
        _ => {}
    }
}

/// Substitutes type parameters in an action block (`Vec::<T>::new()`).
struct TypeSubst<'a>(&'a HashMap<String, syn::Type>);

impl syn::visit_mut::VisitMut for TypeSubst<'_> {
    fn visit_type_mut(&mut self, ty: &mut syn::Type) {
        replace_type(ty, self.0);
        syn::visit_mut::visit_type_mut(self, ty);
    }
}

pub(crate) fn replace_type(ty: &mut syn::Type, subst: &HashMap<String, syn::Type>) {
    match ty {
        syn::Type::Path(type_path) => {
            if type_path.qself.is_none() && type_path.path.segments.len() == 1 {
                let ident = type_path.path.segments[0].ident.to_string();
                if let Some(new_ty) = subst.get(&ident) {
                    *ty = new_ty.clone();
                    return;
                }
            }
            for seg in &mut type_path.path.segments {
                if let syn::PathArguments::AngleBracketed(args) = &mut seg.arguments {
                    for arg in &mut args.args {
                        if let syn::GenericArgument::Type(inner_ty) = arg {
                            replace_type(inner_ty, subst);
                        }
                    }
                }
            }
        }
        syn::Type::Reference(type_ref) => replace_type(&mut type_ref.elem, subst),
        syn::Type::Tuple(type_tuple) => {
            for elem in &mut type_tuple.elems {
                replace_type(elem, subst);
            }
        }
        syn::Type::Array(type_arr) => replace_type(&mut type_arr.elem, subst),
        syn::Type::Slice(type_slice) => replace_type(&mut type_slice.elem, subst),
        syn::Type::Paren(type_paren) => replace_type(&mut type_paren.elem, subst),
        _ => {}
    }
}

/// A terminator whose match is a fixed string, or a few of them - what a
/// scan can *find* rather than *try* at every position. See
/// [`Codegen::scan_terminator`] for what qualifies and
/// [`Codegen::generate_skip_to`] for the parser it becomes.
struct ScanSet {
    /// String and char literals, deduplicated. `"\n"` is folded into
    /// `line_ending` when both are present.
    lits: Vec<String>,
    /// The built-in `line_ending`: `\n`, with one look back for `\r`.
    line_ending: bool,
}

impl ScanSet {
    /// How many distinct byte strings the scan has to look for. The one-pass
    /// scan handles up to three (`memchr3` over the first bytes, then a
    /// check); more than that takes the position-by-position path.
    fn needles(&self) -> usize {
        self.lits.len() + usize::from(self.line_ending)
    }
}

/// What a builtin expects - the text after `expected …`.
fn builtin_expectation(name: &str) -> Option<&'static str> {
    Some(match name {
        "ident" | "raw_ident" => "identifier",
        "string" => "string literal",
        "char" => "character literal",
        "any" => "any character",
        "alpha1" => "letters",
        "digit" => "a digit",
        "digit1" => "digits",
        "hex_digit0" | "hex_digit1" => "hex digits",
        "oct_digit0" | "oct_digit1" => "octal digits",
        "binary_digit0" | "binary_digit1" => "binary digits",
        "space0" | "space1" | "multispace0" | "multispace1" => "whitespace",
        "line_ending" => "line ending",
        "eof" => "end of input",
        "frame_end" => "the frame boundary",
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" => "integer literal",
        "f32" | "f64" => "float literal",
        "bool" => "`true` or `false`",
        _ => return None,
    })
}

/// Substitutes the parser parameters (`subst`) and the type parameters
/// (`type_subst`) in a template. One traversal for both.
pub(crate) fn substitute_pattern(
    pattern: &mut ModelPattern,
    subst: &HashMap<String, ModelPattern>,
    type_subst: &HashMap<String, syn::Type>,
) {
    match pattern {
        ModelPattern::RuleCall {
            rule_path,
            binding,
            generics,
            args,
        } => {
            if let Some(ident) = rule_path.segments.last().map(|s| s.ident.to_string()) {
                if let Some(new_pat) = subst.get(&ident) {
                    let mut cloned = new_pat.clone();
                    // Carry the binding ("elements:") over to the argument when substituting.
                    if binding.is_some() {
                        set_binding(&mut cloned, binding.clone());
                    }
                    *pattern = cloned;
                    return;
                }
            }
            // Not a parameter: then an ordinary call whose own generics and
            // arguments may contain the template parameters
            // (`inner<T>(x=item)`).
            for ty in generics.iter_mut() {
                replace_type(ty, type_subst);
            }
            for arg in args.iter_mut() {
                match arg {
                    Argument::Positional(p) | Argument::Named(_, p) => {
                        substitute_pattern(p, subst, type_subst);
                    }
                }
            }
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for p in seq {
                    substitute_pattern(p, subst, type_subst);
                }
            }
        }
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::Bounded { pattern: inner, .. }
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _)
        | ModelPattern::Count { pattern: inner, .. }
        | ModelPattern::Fold { pattern: inner, .. }
        | ModelPattern::LexicalScope(inner, _)
        | ModelPattern::SpacedScope(inner, _) => {
            substitute_pattern(inner, subst, type_subst);
        }
        ModelPattern::Parenthesized(inner, _)
        | ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _) => {
            for p in inner {
                substitute_pattern(p, subst, type_subst);
            }
        }
        ModelPattern::Recover { body, sync, .. } => {
            substitute_pattern(body, subst, type_subst);
            substitute_pattern(sync, subst, type_subst);
        }
        ModelPattern::Until { pattern: inner, .. } => {
            substitute_pattern(inner, subst, type_subst);
        }
        ModelPattern::Lit { .. } | ModelPattern::Fail { .. } | ModelPattern::Cut(_) => {}
    }
}

pub(crate) fn get_inner_binding(pattern: &ModelPattern) -> Option<&syn::Ident> {
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
        ModelPattern::Bounded { pattern, .. } => get_inner_binding(pattern),
        ModelPattern::SpanBinding(inner, _, _) => get_inner_binding(inner),
        ModelPattern::Recover { binding, .. } => binding.as_ref(),
        ModelPattern::Until { binding, .. } => binding.as_ref(),
        ModelPattern::Count { binding, .. } | ModelPattern::Fold { binding, .. } => {
            binding.as_ref()
        }
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

impl<'a> Codegen<'a> {
    pub fn generate_sequence_steps(
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
                steps.push(quote! { ::winnow_grammar::rt::skip_trivia(WS, #input)?; });
            }

            steps.push(self.generate_step(p, in_cut, is_lexical));
        }
        quote! { #(#steps)* }
    }

    pub fn generate_step(
        &self,
        pattern: &ModelPattern,
        in_cut: bool,
        is_lexical: bool,
    ) -> TokenStream {
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
                // A repetition collects, so the annotation helps inference -
                // unless it is a run of characters, which is text.
                ModelPattern::Repeat(inner, _)
                | ModelPattern::Plus(inner, _)
                | ModelPattern::Bounded { pattern: inner, .. }
                    if !self.is_char_class(inner) =>
                {
                    quote_spanned! {span=>
                        let #name: Vec<_> = #parser_expr.parse_next(#input)?;
                    }
                }
                // A fold's value is the accumulator, whose type comes from
                // `init`, and a count's is the `usize` it mapped to - neither
                // is a collection, so neither must be annotated as one.
                ModelPattern::Fold { .. } | ModelPattern::Count { .. } => quote_spanned! {span=>
                    let #name = #parser_expr.parse_next(#input)?;
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
                ModelPattern::Repeat(_, _)
                | ModelPattern::Plus(_, _)
                | ModelPattern::Bounded { .. } => quote_spanned! {span=>
                    let _: () = #parser_expr.parse_next(#input)?;
                },
                _ => quote_spanned! {span=>
                    let _ = #parser_expr.parse_next(#input)?;
                },
            },
        }
    }

    pub fn generate_delimited_step(
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
            quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(WS, #input)?; }
        } else {
            quote! {}
        };

        let inner_steps = self.generate_sequence_steps(inner, in_cut, is_lexical);

        let inner_triggers_cut = inner.iter().any(|p| matches!(p, ModelPattern::Cut(_)));
        let final_cut = in_cut || inner_triggers_cut;

        // Infix WS between Inner and Close
        let ws_before_close = if !is_lexical {
            quote_spanned! {span=> ::winnow_grammar::rt::skip_trivia(WS, #input)?; }
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

    pub fn generate_argument_expr(&self, arg: &Argument, is_lexical: bool) -> TokenStream {
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

    /// The result type of an argument pattern - for inferring missing type
    /// parameters of a template.
    ///
    /// A literal yields `()`, a user rule its declared return type, a builtin
    /// the type from its declaration. Everything else (groups, repetitions)
    /// remains open - then the caller has to spell out the generics.
    fn infer_type(&self, pattern: &ModelPattern) -> Option<syn::Type> {
        use winnow_grammar_model::Backend;
        match pattern {
            ModelPattern::Lit { .. } => Some(syn::parse_quote!(())),
            ModelPattern::RuleCall { rule_path, .. } => {
                let name = rule_path.segments.last()?.ident.to_string();
                if let Some(rule) = self.grammar.rules.iter().find(|r| r.name == name) {
                    return Some(rule.return_type.clone());
                }
                crate::WinnowBackend::get_builtins()
                    .iter()
                    .find(|b| b.name == name)
                    .and_then(|b| syn::parse_str::<syn::Type>(b.return_type).ok())
            }
            _ => None,
        }
    }

    /// Can this terminator be *scanned* for rather than *tried* at every
    /// position? Only one whose alternatives are all fixed strings can: a
    /// literal, the built-in `line_ending` (two shapes, settled by one look
    /// back), the built-in `eof` (nothing to look for: the scan runs to the
    /// end), or a group of such alternatives - `until(";" | "\n")`. A rule of
    /// the grammar's own under one of those names is that rule, not the
    /// built-in - the same precedence `generate_rule_call_parser` gives it -
    /// and takes the slow path.
    fn scan_terminator(&self, pattern: &ModelPattern) -> Option<ScanSet> {
        let mut set = ScanSet {
            lits: Vec::new(),
            line_ending: false,
        };
        if !self.collect_scan_set(pattern, &mut set) {
            return None;
        }
        if set.line_ending {
            set.lits.retain(|l| l != "\n");
        }
        Some(set)
    }

    fn collect_scan_set(&self, pattern: &ModelPattern, set: &mut ScanSet) -> bool {
        match pattern {
            ModelPattern::Lit { lit, .. } => {
                let text = match lit {
                    syn::Lit::Str(s) => s.value(),
                    syn::Lit::Char(c) => c.value().to_string(),
                    _ => return false,
                };
                if !set.lits.contains(&text) {
                    set.lits.push(text);
                }
                true
            }
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                ..
            } if generics.is_empty() && args.is_empty() => {
                let Some(name) = rule_path.segments.last().map(|s| s.ident.to_string()) else {
                    return false;
                };
                if self.user_rules.contains(&name) {
                    // A rule that matches nothing but literals is scanned for
                    // as those literals. Only a lexical one qualifies - a
                    // syntactic rule begins with whitespace, so it does not
                    // start where its literal does. See
                    // `analysis::literal_rules`.
                    let Some(lits) = self.literal_rules.get(&name) else {
                        return false;
                    };
                    for l in lits {
                        if !set.lits.contains(l) {
                            set.lits.push(l.clone());
                        }
                    }
                    return true;
                }
                match name.as_str() {
                    "line_ending" => {
                        set.line_ending = true;
                        true
                    }
                    "eof" => true,
                    winnow_grammar_model::frame::FRAME_END => {
                        // Resolved for this rule by the frame check.
                        let Some(b) = self.current_boundary.borrow().clone() else {
                            return false;
                        };
                        if !set.lits.contains(&b) {
                            set.lits.push(b);
                        }
                        true
                    }
                    _ => false,
                }
            }
            ModelPattern::Group { alts, .. } => {
                alts.iter().all(|(seq, _, _)| match seq.as_slice() {
                    [single] => self.collect_scan_set(single, set),
                    _ => false,
                })
            }
            _ => false,
        }
    }

    /// The parser that consumes everything before `terminator` and yields it
    /// as `&'a str`, without consuming the terminator. Never fails: with no
    /// terminator ahead it consumes to the end of the input.
    ///
    /// This is the skip in `until(…)` and in `recover(…)`, so both get the same
    /// value and the same fast path. A terminator the scan can find is found
    /// in one pass with `memchr`; anything else has to be tried at every
    /// position, a parser call per character. What a terminator means does
    /// not depend on where the rule is used: a `#[frame]` elsewhere in the
    /// grammar changes nothing here (see `winnow_grammar_model::frame`).
    fn generate_skip_to(&self, terminator: &ModelPattern, is_lexical: bool) -> TokenStream {
        let span = Span::mixed_site();
        let set = self
            .scan_terminator(terminator)
            .filter(|s| s.needles() <= 3);
        match set {
            Some(set) if set.needles() == 0 => quote_spanned! {span=> ::winnow::token::rest },
            Some(set) if set.needles() == 1 && set.line_ending => {
                quote_spanned! {span=> ::winnow_grammar::rt::scan_to_line_ending() }
            }
            Some(set) if set.needles() == 1 => {
                let lit = &set.lits[0];
                quote_spanned! {span=> ::winnow_grammar::rt::scan_to_literal(#lit) }
            }
            Some(set) => {
                let lits = &set.lits;
                let line_ending = set.line_ending;
                quote_spanned! {span=> ::winnow_grammar::rt::scan_to_any(&[#(#lits),*], #line_ending) }
            }
            None => {
                let p = self.generate_parser_expr(terminator, is_lexical, false);
                quote_spanned! {span=>
                    ::winnow::Parser::take(::winnow::combinator::repeat::<_, _, (), _, _>(0.., (
                        ::winnow::combinator::not(::winnow::combinator::peek(#p)),
                        ::winnow::token::any
                    )))
                }
            }
        }
    }

    pub fn generate_rule_call_parser(
        &self,
        rule_path: &syn::Path,
        call_generics: &[syn::Type],
        args: &[Argument],
        is_lexical: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let rule_name = &rule_path.segments.last().unwrap().ident;
        let name_str = rule_name.to_string();

        if self.user_rules.contains(&name_str) {
            // 1. Find the target rule in the grammar
            let target_rule = self
                .grammar
                .rules
                .iter()
                .find(|r| r.name == name_str)
                .unwrap();

            if super::is_template(target_rule) {
                // 3b. Parser parameters -> argument patterns
                let arg_patterns: Vec<ModelPattern> = args
                    .iter()
                    .map(|arg| match arg {
                        Argument::Positional(p) | Argument::Named(_, p) => p.clone(),
                    })
                    .collect();
                let mut subst = HashMap::new();
                for (param, arg_pattern) in target_rule.params.iter().zip(&arg_patterns) {
                    subst.insert(param.name.to_string(), arg_pattern.clone());
                }

                // 3a. Type parameters -> type. Explicitly given ones
                // (`list<u32>(…)`) win; missing ones are inferred from the
                // argument at the same position (`list(item=u32)` -> T = u32).
                // The same convention as syn-grammar's monomorphizer: the i-th
                // type parameter belongs to the i-th parser parameter.
                let mut type_subst = HashMap::new();
                let type_params = target_rule.generics.params.iter().filter_map(|g| match g {
                    syn::GenericParam::Type(t) => Some(t.ident.to_string()),
                    _ => None,
                });
                for (idx, name) in type_params.enumerate() {
                    if let Some(call_ty) = call_generics.get(idx) {
                        type_subst.insert(name, call_ty.clone());
                    } else if let Some(ty) = arg_patterns.get(idx).and_then(|p| self.infer_type(p))
                    {
                        type_subst.insert(name, ty);
                    }
                }

                // 4. Clone the rule's AST, substitute parameters and types -
                //    in patterns AND in the action blocks (`Vec::<T>::new()`).
                let mut inlined_variants = target_rule.variants.clone();
                for variant in &mut inlined_variants {
                    for step in &mut variant.pattern {
                        substitute_pattern(step, &subst, &type_subst);
                    }
                    // The action block comes without its braces; for the type
                    // substitution it is parsed as a block and written back
                    // with braces - a block in expression position is valid
                    // everywhere the tokens were before.
                    let action = &variant.action;
                    if let Ok(mut block) = syn::parse2::<syn::Block>(quote::quote!({ #action })) {
                        syn::visit_mut::VisitMut::visit_block_mut(
                            &mut TypeSubst(&type_subst),
                            &mut block,
                        );
                        variant.action = quote::quote!(#block);
                    }
                }

                // 5. Compile the inlined parser body directly (incl. type generics in the return type)
                let mut ret_type = target_rule.return_type.clone();
                replace_type(&mut ret_type, &type_subst);

                let combined_lexical =
                    is_lexical || target_rule.is_lexical || target_rule.name == "WS";
                let body = self.generate_variants_body(
                    &inlined_variants,
                    &ret_type,
                    combined_lexical,
                    true,
                );
                let inner_err_type = quote_spanned! {span=> ::winnow::error::ErrMode<E> };
                let input_var = &self.input_ident; // <-- NEW: use the defined identifier

                return quote_spanned! {span=>
                    (|#input_var: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #inner_err_type> {
                        let mut parser = (|#input_var: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #inner_err_type> {
                            #body
                        });
                        ::winnow::Parser::parse_next(&mut parser, #input_var)
                    })
                };
            }

            // --- Normal function call for NON-template rules ---
            let fn_name = quote::format_ident!("parse_{}_inner", rule_name, span = span);
            if args.is_empty() {
                return quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| #fn_name(i)) };
            } else {
                let arg_exprs = args
                    .iter()
                    .map(|arg| self.generate_argument_expr(arg, is_lexical));
                return quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| #fn_name(i, #(#arg_exprs),*)) };
            }
        }

        let inner_err_type = quote_spanned! {span=> ::winnow::error::ErrMode<E> };
        let input_type = quote_spanned! {span=> ::winnow_grammar::ParseInput<'a, S> };

        // `raw_ident`'s characters, shared with `ident` - which is
        // `intern(raw_ident)` and nothing else (ADR 18 §1).
        let raw_ident = quote_spanned! {span=>
            ::winnow_grammar::rt::class_or_wide::<S, E>(
                ::winnow_grammar::ascii::AsciiClass::IDENT,
                |c| c.is_alphanumeric(),
                1,
            )
        };

        // `interner I;` sends `ident` and `intern(…)` to the declared interner
        // instead of the context's - ADR 22.
        let intern_with = |inner: &TokenStream| match &self.grammar.interner {
            Some(ty) => quote_spanned! {span=>
                ::winnow_grammar::rt::intern_in::<#ty, _, _, _, _>(#inner)
            },
            None => quote_spanned! {span=> ::winnow_grammar::rt::intern(#inner) },
        };

        let p = match name_str.as_str() {
            "raw_ident" => raw_ident,
            "ident" => intern_with(&raw_ident),
            // `intern(p)`: the one builtin that takes an argument. The
            // argument is an ordinary pattern, so `intern(until(";"))` and
            // `intern(my_rule)` are the same shape as `intern(string)`.
            // `text(p)`: run `p` and hand back the input it consumed. winnow's
            // `.take()` - the value `p` produced is dropped, the span is not.
            // No allocation: the result borrows the input.
            //
            // Several patterns are a sequence, not several arguments:
            // `text(digit{2} raw_ident)` captures both, and through the
            // ordinary sequence generator, so the whitespace between them is
            // whatever it would be outside the `text`.
            "text" => {
                if args.is_empty() {
                    let msg = "`text` takes the pattern whose input to capture, and got none";
                    return syn::Error::new(syn::spanned::Spanned::span(rule_path), msg)
                        .to_compile_error();
                }
                let seq: Vec<ModelPattern> = args
                    .iter()
                    .map(|a| match a {
                        Argument::Positional(p) | Argument::Named(_, p) => p.clone(),
                    })
                    .collect();
                // A run of a character class already *is* its text, so
                // `text(digit{1,2})` is that run and not a wrapper around it.
                // Taking it twice measured ~3 ns more on the 1BRC temperature.
                if let [only] = &seq[..] {
                    if self.is_char_run(only) {
                        return self.generate_parser_expr(only, is_lexical, false);
                    }
                }
                let inner = self.generate_sequence_parser_with(&seq, is_lexical, true);
                quote_spanned! {span=> ::winnow::Parser::take(#inner) }
            }
            // `dec<T>(p)`: the text `p` matched, read as a number.
            //
            // `try_map` rather than winnow's `parse_to`: the latter throws the
            // `FromStr` error away and reports a bare position, so a value too
            // large for `T` would fail without saying why - and saying why is
            // most of the point. `ParseError` turns an external error into its
            // message (`FromExternalError`), so "number too large to fit in
            // target type" arrives intact.
            "dec" => {
                if args.is_empty() {
                    let msg = "`dec` takes the pattern whose text to read, and got none";
                    return syn::Error::new(syn::spanned::Spanned::span(rule_path), msg)
                        .to_compile_error();
                }
                let seq: Vec<ModelPattern> = args
                    .iter()
                    .map(|a| match a {
                        Argument::Positional(p) | Argument::Named(_, p) => p.clone(),
                    })
                    .collect();
                // As in `text`: a run is its own text, so it is read as it
                // stands rather than taken a second time.
                let taken = match &seq[..] {
                    [only] if self.is_char_run(only) => {
                        self.generate_parser_expr(only, is_lexical, false)
                    }
                    _ => {
                        let inner = self.generate_sequence_parser_with(&seq, is_lexical, true);
                        quote_spanned! {span=> ::winnow::Parser::take(#inner) }
                    }
                };
                match call_generics.first() {
                    Some(ty) => quote_spanned! {span=>
                        ::winnow_grammar::rt::dec::<_, #ty, _, _>(#taken)
                    },
                    None => quote_spanned! {span=> ::winnow_grammar::rt::dec(#taken) },
                }
            }
            "intern" => match args {
                [arg] => {
                    let inner = self.generate_argument_expr(arg, is_lexical);
                    intern_with(&inner)
                }
                _ => {
                    let msg = format!(
                        "`intern` takes exactly one pattern to intern, got {}",
                        args.len()
                    );
                    return syn::Error::new(syn::spanned::Spanned::span(rule_path), msg)
                        .to_compile_error();
                }
            },
            "string" => quote_spanned! {span=>
                 delimited(
                    '"',
                    ::winnow::ascii::take_escaped(
                        ::winnow::token::none_of(['\\', '"']),
                        '\\',
                        ::winnow::token::one_of(['\\', '"'])
                    ),
                    '"'.context(::winnow::error::StrContext::Expected(::winnow::error::StrContextValue::CharLiteral('"')))
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
                                '\"' => '\"',
                                '0' => '\0',
                                _ => c // fallback
                             }
                        }),
                        ::winnow::token::none_of(['\\', '\''])
                    )),
                    '\''.context(::winnow::error::StrContext::Expected(::winnow::error::StrContextValue::CharLiteral('\'')))
                )
            },
            "any" => quote_spanned! {span=> ::winnow::token::any::<#input_type, #inner_err_type> },
            "alpha1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::ALPHA,
                    1,
                )
            },
            // A single digit, as opposed to `digit1`'s greedy run of them.
            // Fixed-width numeric formats are written with it and a bounded
            // repetition (`digit{1,2}`), which a greedy terminal cannot express.
            "digit" => quote_spanned! {span=>
                ::winnow::token::one_of::<#input_type, _, #inner_err_type>('0'..='9')
            },
            "digit1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::DIGIT,
                    1,
                )
            },
            "hex_digit0" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::HEX_DIGIT,
                    0,
                )
            },
            "hex_digit1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::HEX_DIGIT,
                    1,
                )
            },
            "oct_digit0" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::OCT_DIGIT,
                    0,
                )
            },
            "oct_digit1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::OCT_DIGIT,
                    1,
                )
            },
            "binary_digit0" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::BINARY_DIGIT,
                    0,
                )
            },
            "binary_digit1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::BINARY_DIGIT,
                    1,
                )
            },
            "space0" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::SPACE,
                    0,
                )
            },
            "space1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::SPACE,
                    1,
                )
            },
            "multispace0" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::MULTISPACE,
                    0,
                )
            },
            "multispace1" => quote_spanned! {span=>
                ::winnow_grammar::rt::class::<S, E>(
                    ::winnow_grammar::ascii::AsciiClass::MULTISPACE,
                    1,
                )
            },
            "line_ending" => {
                quote_spanned! {span=> ::winnow::ascii::line_ending::<#input_type, #inner_err_type> }
            }
            "empty" => quote_spanned! {span=> ::winnow::combinator::empty },
            "eof" => quote_spanned! {span=> ::winnow::combinator::eof },
            // The boundary of the frame this rule is reached from, resolved
            // by the frame check; the check rejects a `frame_end` no frame
            // reaches, so the fallback only keeps the generator total.
            winnow_grammar_model::frame::FRAME_END => {
                let b = self.current_boundary.borrow().clone().unwrap_or_default();
                quote_spanned! {span=>
                    literal(#b)
                        .context(::winnow::error::StrContext::Expected(::winnow::error::StrContextValue::StringLiteral(#b)))
                }
            }

            "u8" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u8, #inner_err_type> }
            }
            "u16" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u16, #inner_err_type> }
            }
            "u32" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u32, #inner_err_type> }
            }
            "u64" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u64, #inner_err_type> }
            }
            "u128" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u128, #inner_err_type> }
            }
            "usize" => {
                quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, usize, #inner_err_type> }
            }
            "i8" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i8, #inner_err_type> }
            }
            "i16" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i16, #inner_err_type> }
            }
            "i32" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i32, #inner_err_type> }
            }
            "i64" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i64, #inner_err_type> }
            }
            "i128" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i128, #inner_err_type> }
            }
            "isize" => {
                quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, isize, #inner_err_type> }
            }
            "f32" => {
                quote_spanned! {span=> ::winnow::ascii::float::<#input_type, f32, #inner_err_type> }
            }
            "f64" => {
                quote_spanned! {span=> ::winnow::ascii::float::<#input_type, f64, #inner_err_type> }
            }
            "bool" => quote_spanned! {span=>
                ::winnow::combinator::alt((
                    ::winnow::token::literal("true").map(|_| true),
                    ::winnow::token::literal("false").map(|_| false),
                ))
            },
            _ => {
                if args.is_empty() {
                    // A hand-written parser returns `ParseError` whatever the
                    // grammar's error type is; the fast pass drops it.
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow::Parser::parse_next(&mut #rule_path, i).map_err(|e| ::winnow::error::ErrMode::Backtrack(<E as ::winnow_grammar::Diagnostics>::from_parse_error(e)))) }
                } else {
                    let arg_exprs = args
                        .iter()
                        .map(|arg| self.generate_argument_expr(arg, is_lexical));
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| #rule_path(i, #(#arg_exprs),*).map_err(|e| ::winnow::error::ErrMode::Backtrack(<E as ::winnow_grammar::Diagnostics>::from_parse_error(e)))) }
                }
            }
        };

        // winnow's primitives only report the position. The expectation comes
        // from here - otherwise an `ident` branch would contribute nothing at
        // all to `expected one of: …`.
        match builtin_expectation(&name_str) {
            Some(what) => quote_spanned! {span=> ::winnow_grammar::rt::expected(#what, #p) },
            None => p,
        }
    }

    /// Does this pattern match exactly one character of a built-in class?
    ///
    /// A repetition of one is a **run** of characters, and a run is text. The
    /// `{n,m}` syntax is borrowed from regular expressions, where `\d{1,2}`
    /// matches text and not a list of characters, and a `Vec<char>` there is
    /// a copy of input the parser already walked over. Every other repetition
    /// yields its elements, because those are values the parser built.
    ///
    /// The list is written out rather than read off the built-in table,
    /// because the property is not "yields a `char`". `char` yields one and
    /// is **not** here: it parses a character *literal*, so `'\n'` is four
    /// characters of input and one of value - its text and its value are
    /// different things, and taking the text would hand back the escape
    /// rather than what it stands for. A new built-in belongs here only if it
    /// consumes exactly one character and yields that character unchanged.
    ///
    /// Shadowing counts: a grammar that defines its own `digit` means that
    /// one, and it is a rule like any other.
    fn is_char_class(&self, p: &ModelPattern) -> bool {
        const CHAR_CLASSES: &[&str] = &["digit", "any"];
        let ModelPattern::RuleCall {
            rule_path, args, ..
        } = p
        else {
            return false;
        };
        if !args.is_empty() {
            return false;
        }
        let Some(name) = rule_path.get_ident().map(|i| i.to_string()) else {
            return false;
        };
        if self.user_rules.contains(&name) {
            return false;
        }
        CHAR_CLASSES.contains(&name.as_str())
    }

    /// A repetition whose element is a character class - the patterns that
    /// yield `&'a str` rather than their elements. `text(..)` and `dec(..)`
    /// over one of these have nothing to add: the run is already the text.
    fn is_char_run(&self, p: &ModelPattern) -> bool {
        match p {
            ModelPattern::Repeat(inner, _) | ModelPattern::Plus(inner, _) => {
                self.is_char_class(inner)
            }
            ModelPattern::Bounded { pattern, .. } => self.is_char_class(pattern),
            _ => false,
        }
    }

    /// `x*`, `x+` and `x{n,m}`: one generator, three spellings of the bounds.
    ///
    /// What it yields depends on what is repeated and on whether anyone named
    /// it. A run of characters ([`is_char_class`](Self::is_char_class)) is the
    /// text it matched - in a syntactic rule that includes the whitespace
    /// between the elements, because that is what the parser consumed, which
    /// is one more reason a numeric run belongs in a lexical rule. Anything
    /// else yields its elements. A repetition nobody named yields nothing and
    /// collects nothing either way.
    fn generate_repetition(
        &self,
        inner: &ModelPattern,
        min: usize,
        max: Option<usize>,
        is_lexical: bool,
        is_discarded: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let p = self.generate_parser_expr(inner, is_lexical, false);
        let elem = if is_lexical {
            quote_spanned! {span=> #p }
        } else {
            // `WS` is a function and `preceded` wants a parser.
            quote_spanned! {span=> ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i), #p) }
        };
        let max = match max {
            Some(m) => quote_spanned! {span=> ::core::option::Option::Some(#m) },
            None => quote_spanned! {span=> ::core::option::Option::None },
        };
        let counting = quote_spanned! {span=> ::winnow_grammar::rt::repeat_counting_bounded(#min, #max, #elem) };
        if is_discarded {
            // `take` and throw the slice away, rather than `.map(|_| ())` over
            // the same loop. Both discard the count and both run element for
            // element, but the mapped form measured **2.7x** the taken one on
            // 200_000 digits (500 us against 184; `benches/repetition.rs`),
            // so the cheaper spelling of "nothing" is the one that also
            // produces a value.
            quote_spanned! {span=> ::winnow::Parser::void(::winnow::Parser::take(#counting)) }
        } else if self.is_char_class(inner) {
            quote_spanned! {span=> ::winnow::Parser::take(#counting) }
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::repeat_recording_bounded(#min, #max, #elem) }
        }
    }

    pub fn generate_parser_expr(
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
                rule_path,
                generics,
                args,
                ..
            } => self.generate_rule_call_parser(rule_path, generics, args, is_lexical),
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
                quote_spanned! {span=> ::winnow_grammar::rt::opt_recording(#p) }
            }
            ModelPattern::Repeat(inner, _span) => {
                self.generate_repetition(inner, 0, None, is_lexical, is_discarded)
            }
            ModelPattern::Plus(inner, _span) => {
                self.generate_repetition(inner, 1, None, is_lexical, is_discarded)
            }
            ModelPattern::Bounded {
                pattern, min, max, ..
            } => self.generate_repetition(pattern, *min, *max, is_lexical, is_discarded),
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
                // Skipping to the synchronization point is the expensive half of
                // recovery: it is what runs over the broken region, and it is
                // reached exactly when a file has many errors.
                let skip = self.generate_skip_to(sync, is_lexical);
                quote_spanned! {span=>
                    ::winnow_grammar::rt::recover_recording(
                        #body_parser,
                        #skip,
                        #sync_parser,
                    )
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
            ModelPattern::Until { pattern, .. } => self.generate_skip_to(pattern, is_lexical),
            ModelPattern::Count { pattern, .. } => {
                let p = self.generate_parser_expr(pattern, is_lexical, false);
                if !is_lexical {
                    quote_spanned! {span=> ::winnow_grammar::rt::repeat_counting(0, ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i), #p)) }
                } else {
                    quote_spanned! {span=> ::winnow_grammar::rt::repeat_counting(0, #p) }
                }
            }
            ModelPattern::Fold {
                pattern,
                init,
                step,
                merge,
                ..
            } => {
                let p = self.generate_parser_expr(pattern, is_lexical, false);
                // The fold of a `par_fold` rule leaves a trail for the replay
                // of a failed fast pass (`rt::entry_framed`).
                let fold = if merge.is_some() {
                    quote_spanned! {span=> ::winnow_grammar::rt::par_fold_recording }
                } else {
                    quote_spanned! {span=> ::winnow_grammar::rt::fold_recording }
                };
                if !is_lexical {
                    quote_spanned! {span=> #fold(0, ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i), #p), #init, #step) }
                } else {
                    quote_spanned! {span=> #fold(0, #p, #init, #step) }
                }
            }
            ModelPattern::Fail { message, .. } => match message {
                Some(msg) => quote_spanned! {span=> ::winnow_grammar::rt::fail(#msg) },
                None => quote_spanned! {span=> ::winnow_grammar::rt::fail("Explicit failure") },
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

    pub fn generate_sequence_parser(&self, seq: &[ModelPattern], is_lexical: bool) -> TokenStream {
        self.generate_sequence_parser_with(seq, is_lexical, false)
    }

    /// [`generate_sequence_parser`](Self::generate_sequence_parser), with
    /// `discard` saying whether anyone will look at the values.
    ///
    /// `text(p)` and `dec(p)` keep only the span, so their elements are
    /// generated as discarded - which is what stops a `text(digit{1,2})` from
    /// collecting two `char`s into a `Vec` and then throwing it away.
    pub fn generate_sequence_parser_with(
        &self,
        seq: &[ModelPattern],
        is_lexical: bool,
        discard: bool,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let mut parsers = Vec::new();
        let mut in_cut = false;

        for (i, p) in seq.iter().enumerate() {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
                if i > 0 && !is_lexical {
                    parsers.push(quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i)) });
                }
                let p_expr = self.generate_parser_expr(p, is_lexical, discard);
                if in_cut {
                    parsers.push(quote_spanned! {span=> ::winnow::combinator::cut_err(#p_expr) });
                } else {
                    parsers.push(p_expr);
                }
                continue;
            }

            // Infix WS
            if i > 0 && !is_lexical {
                parsers.push(
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i)) },
                );
            }

            let p_expr = self.generate_parser_expr(p, is_lexical, discard);
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

    pub fn generate_delimited_expr(
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
                delimited(#open_p, preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i), #inner_parser), preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow_grammar::rt::skip_trivia(WS, i), #close_p))
            }
        }
    }
}
