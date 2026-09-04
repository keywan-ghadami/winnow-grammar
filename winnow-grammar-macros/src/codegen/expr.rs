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
        ModelPattern::Count { binding, .. } => *binding = new_binding,
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::LexicalScope(inner, _)
        | ModelPattern::SpacedScope(inner, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _) => set_binding(inner, new_binding),
        ModelPattern::Parenthesized(inner, _)
        | ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _) => {
            // Nur bei genau einem Element ist eindeutig, worauf sich die
            // Bindung bezieht.
            if let [single] = inner.as_mut_slice() {
                set_binding(single, new_binding);
            }
        }
        _ => {}
    }
}

/// Ersetzt Typparameter in einem Aktionsblock (`Vec::<T>::new()`).
struct TypSubst<'a>(&'a HashMap<String, syn::Type>);

impl syn::visit_mut::VisitMut for TypSubst<'_> {
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

/// Setzt in einer Vorlage die Parser-Parameter (`subst`) und die Typparameter
/// (`type_subst`) ein. Eine Traversierung fuer beides.
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
                    // Zuweisung ("elements:") beim Ersetzen auf das Argument uebertragen.
                    if binding.is_some() {
                        set_binding(&mut cloned, binding.clone());
                    }
                    *pattern = cloned;
                    return;
                }
            }
            // Kein Parameter: dann ein gewoehnlicher Aufruf, dessen eigene
            // Generics und Argumente die Vorlagenparameter enthalten koennen
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
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _)
        | ModelPattern::Count { pattern: inner, .. }
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
                steps.push(quote! { let _ = WS(#input)?; });
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
                return self.generate_delimited_step(inner, "{", "]", in_cut, is_lexical)
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
            quote_spanned! {span=> let _ = WS(#input)?; }
        } else {
            quote! {}
        };

        let inner_steps = self.generate_sequence_steps(inner, in_cut, is_lexical);

        let inner_triggers_cut = inner.iter().any(|p| matches!(p, ModelPattern::Cut(_)));
        let final_cut = in_cut || inner_triggers_cut;

        // Infix WS between Inner and Close
        let ws_before_close = if !is_lexical {
            quote_spanned! {span=> let _ = WS(#input)?; }
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

    /// Der Ergebnistyp eines Argumentmusters - fuer die Ableitung fehlender
    /// Typparameter einer Vorlage.
    ///
    /// Ein Literal liefert `()`, eine Nutzerregel ihren deklarierten
    /// Rueckgabetyp, ein Builtin den Typ aus seiner Deklaration. Alles andere
    /// (Gruppen, Wiederholungen) bleibt offen - dann muss der Aufrufer die
    /// Generics ausschreiben.
    fn leite_typ_ab(&self, pattern: &ModelPattern) -> Option<syn::Type> {
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
            // 1. Zielregel in der Grammatik finden
            let target_rule = self
                .grammar
                .rules
                .iter()
                .find(|r| r.name == name_str)
                .unwrap();

            if super::ist_vorlage(target_rule) {
                // 3b. Parser-Parameter -> Argumentmuster
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

                // 3a. Typparameter -> Typ. Explizit angegebene (`list<u32>(…)`)
                // gewinnen; fehlende werden aus dem Argument an derselben
                // Position abgeleitet (`list(item=u32)` -> T = u32). Dieselbe
                // Konvention wie syn-grammars Monomorphizer: der i-te
                // Typparameter gehoert zum i-ten Parser-Parameter.
                let mut type_subst = HashMap::new();
                let type_params = target_rule.generics.params.iter().filter_map(|g| match g {
                    syn::GenericParam::Type(t) => Some(t.ident.to_string()),
                    _ => None,
                });
                for (idx, name) in type_params.enumerate() {
                    if let Some(call_ty) = call_generics.get(idx) {
                        type_subst.insert(name, call_ty.clone());
                    } else if let Some(ty) =
                        arg_patterns.get(idx).and_then(|p| self.leite_typ_ab(p))
                    {
                        type_subst.insert(name, ty);
                    }
                }

                // 4. AST der Regel klonen, Parameter und Typen ersetzen -
                //    in Mustern UND in den Aktionsbloecken (`Vec::<T>::new()`).
                let mut inlined_variants = target_rule.variants.clone();
                for variant in &mut inlined_variants {
                    for step in &mut variant.pattern {
                        substitute_pattern(step, &subst, &type_subst);
                    }
                    // Der Aktionsblock liegt ohne seine Klammern vor; fuer die
                    // Typersetzung wird er als Block geparst und mit Klammern
                    // zurueckgeschrieben - ein Block in Ausdrucksposition ist
                    // ueberall gueltig, wo die Tokens vorher standen.
                    let action = &variant.action;
                    if let Ok(mut block) = syn::parse2::<syn::Block>(quote::quote!({ #action })) {
                        syn::visit_mut::VisitMut::visit_block_mut(
                            &mut TypSubst(&type_subst),
                            &mut block,
                        );
                        variant.action = quote::quote!(#block);
                    }
                }

                // 5. Inlined Parser-Body direkt kompilieren (inkl. Typ-Generics in Return-Type)
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
                let inner_err_type = quote_spanned! {span=> ::winnow::error::ErrMode<::winnow::error::ContextError> };
                let input_var = &self.input_ident; // <-- NEU: Beziehe den definierten Identifier

                return quote_spanned! {span=>
                    (|#input_var: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #inner_err_type> {
                        let mut parser = (|#input_var: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #inner_err_type> {
                            #body
                        });
                        ::winnow::Parser::parse_next(&mut parser, #input_var)
                    })
                };
            }

            // --- Normaler Funktionsaufruf für NICHT-Template Regeln ---
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

        let inner_err_type =
            quote_spanned! {span=> ::winnow::error::ErrMode<::winnow::error::ContextError> };
        let input_type = quote_spanned! {span=> ::winnow_grammar::ParseInput<'a, S> };

        match name_str.as_str() {
            "raw_ident" => quote_spanned! {span=>
                ::winnow::token::take_while(1.., |c| ::winnow::stream::AsChar::as_char(c).is_alphanumeric() || ::winnow::stream::AsChar::as_char(c) == '_')
            },
            "ident" => quote_spanned! {span=>
                (|input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<_, ::winnow::error::ErrMode<::winnow::error::ContextError>> {
                    let s: &str = ::winnow::token::take_while(1.., |c| ::winnow::stream::AsChar::as_char(c).is_alphanumeric() || ::winnow::stream::AsChar::as_char(c) == '_').parse_next(input)?;
                    let symbol = input.state.interner.intern_string(s);
                    Ok(symbol)
                })
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
                                '\"' => '\"',
                                '0' => '\0',
                                _ => c // fallback
                             }
                        }),
                        ::winnow::token::none_of(['\\', '\''])
                    )),
                    '\''
                )
            },
            "any" => quote_spanned! {span=> ::winnow::token::any::<#input_type, #inner_err_type> },
            "alpha1" => {
                quote_spanned! {span=> ::winnow::ascii::alpha1::<#input_type, #inner_err_type> }
            }
            "digit1" => {
                quote_spanned! {span=> ::winnow::ascii::digit1::<#input_type, #inner_err_type> }
            }
            "hex_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit0::<#input_type, #inner_err_type> }
            }
            "hex_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit1::<#input_type, #inner_err_type> }
            }
            "oct_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit0::<#input_type, #inner_err_type> }
            }
            "oct_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit1::<#input_type, #inner_err_type> }
            }
            "binary_digit0" => quote_spanned! {span=>
                ::winnow::token::take_while(0.., |c| c == '0' || c == '1')
            },
            "binary_digit1" => quote_spanned! {span=>
                ::winnow::token::take_while(1.., |c| c == '0' || c == '1')
            },
            "space0" => {
                quote_spanned! {span=> ::winnow::ascii::space0::<#input_type, #inner_err_type> }
            }
            "space1" => {
                quote_spanned! {span=> ::winnow::ascii::space1::<#input_type, #inner_err_type> }
            }
            "multispace0" => {
                quote_spanned! {span=> ::winnow::ascii::multispace0::<#input_type, #inner_err_type> }
            }
            "multispace1" => {
                quote_spanned! {span=> ::winnow::ascii::multispace1::<#input_type, #inner_err_type> }
            }
            "line_ending" => {
                quote_spanned! {span=> ::winnow::ascii::line_ending::<#input_type, #inner_err_type> }
            }
            "empty" => quote_spanned! {span=> ::winnow::combinator::empty },
            "eof" => quote_spanned! {span=> ::winnow::combinator::eof },

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
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow::Parser::parse_next(&mut #rule_path, i).map_err(::winnow::error::ErrMode::Backtrack)) }
                } else {
                    let arg_exprs = args
                        .iter()
                        .map(|arg| self.generate_argument_expr(arg, is_lexical));
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| #rule_path(i, #(#arg_exprs),*).map_err(::winnow::error::ErrMode::Backtrack)) }
                }
            }
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
                quote_spanned! {span=> opt(#p) }
            }
            ModelPattern::Repeat(inner, _span) => {
                let p = self.generate_parser_expr(inner, is_lexical, false);
                if !is_lexical {
                    if is_discarded {
                        // Using |i: &mut _| WS(i) explicitly since WS is a function and preceded requires a Parser.
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, (), _, _>(0.., ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #p)) }
                    } else {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(0.., ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #p)) }
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
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, (), _, _>(1.., ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #p)) }
                    } else {
                        quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(1.., ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #p)) }
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
                    quote_spanned! {span=> ::winnow::combinator::repeat::<_, _, Vec<_>, _, _>(0.., ::winnow::combinator::preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #p)).map(|v: Vec<_>| v.len()) }
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

    pub fn generate_sequence_parser(&self, seq: &[ModelPattern], is_lexical: bool) -> TokenStream {
        let span = Span::mixed_site();
        let mut parsers = Vec::new();
        let mut in_cut = false;

        for (i, p) in seq.iter().enumerate() {
            if let ModelPattern::Cut(_) = p {
                in_cut = true;
                if i > 0 && !is_lexical {
                    parsers.push(quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i)) });
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
                parsers.push(
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i)) },
                );
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
                delimited(#open_p, preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #inner_parser), preceded(|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i), #close_p))
            }
        }
    }
}
