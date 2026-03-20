use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn_grammar_model::model::{Argument, ModelPattern};
use std::collections::HashMap;

pub(crate) fn set_binding(pattern: &mut ModelPattern, new_binding: Option<syn::Ident>) {
    match pattern {
        ModelPattern::RuleCall { binding, .. } => *binding = new_binding,
        ModelPattern::Group { binding, .. } => *binding = new_binding,
        ModelPattern::Lit { binding, .. } => *binding = new_binding,
        ModelPattern::Recover { binding, .. } => *binding = new_binding,
        ModelPattern::Until { binding, .. } => *binding = new_binding,
        ModelPattern::Count { binding, .. } => *binding = new_binding,
        ModelPattern::Optional(inner, _) |
        ModelPattern::Repeat(inner, _) |
        ModelPattern::Plus(inner, _) |
        ModelPattern::SpanBinding(inner, _, _) |
        ModelPattern::LexicalScope(inner, _) |
        ModelPattern::SpacedScope(inner, _) |
        ModelPattern::Peek(inner, _) |
        ModelPattern::Not(inner, _) => set_binding(inner, new_binding),
        ModelPattern::Parenthesized(inner, _) |
        ModelPattern::Bracketed(inner, _) |
        ModelPattern::Braced(inner, _) => {
            if inner.len() == 1 { set_binding(&mut inner[0], new_binding); }
        }
        _ => {}
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
            for elem in &mut type_tuple.elems { replace_type(elem, subst); }
        }
        syn::Type::Array(type_arr) => replace_type(&mut type_arr.elem, subst),
        syn::Type::Slice(type_slice) => replace_type(&mut type_slice.elem, subst),
        syn::Type::Paren(type_paren) => replace_type(&mut type_paren.elem, subst),
        _ => {}
    }
}

pub(crate) fn substitute_pattern(pattern: &mut ModelPattern, subst: &HashMap<String, ModelPattern>) {
    match pattern {
        ModelPattern::RuleCall { rule_path, binding, .. } => {
            if let Some(ident) = rule_path.segments.last().map(|s| s.ident.to_string()) {
                if let Some(new_pat) = subst.get(&ident) {
                    let mut cloned = new_pat.clone();
                    // Zuweisung ("elements:") beim Ersetzen auf das Argument übertragen!
                    if binding.is_some() {
                        set_binding(&mut cloned, binding.clone());
                    }
                    *pattern = cloned;
                    return;
                }
            }
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for p in seq {
                    substitute_pattern(p, subst);
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
            substitute_pattern(inner, subst);
        }
        ModelPattern::Parenthesized(inner, _)
        | ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _) => {
            for p in inner {
                substitute_pattern(p, subst);
            }
        }
        ModelPattern::Recover { body, sync, .. } => {
            substitute_pattern(body, subst);
            substitute_pattern(sync, subst);
        }
        ModelPattern::Until { pattern: inner, .. } => {
            substitute_pattern(inner, subst);
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
            let target_rule = self.grammar.rules.iter().find(|r| r.name == name_str).unwrap();
            
            // 2. Prüfen, ob es sich um eine Template-Regel handelt
            let is_template = target_rule.params.iter().any(|p| {
                if let Some(syn::Type::Path(type_path)) = &p.ty {
                    if let Some(segment) = type_path.path.segments.last() {
                        return segment.ident == "Rule";
                    }
                }
                false
            });

            if is_template {
                // 3a. Typ-Generics Substitutions-Map aufbauen (z.B. T -> u32)
                let mut type_subst = HashMap::new();
                for (idx, gen_param) in target_rule.generics.params.iter().enumerate() {
                    if let syn::GenericParam::Type(ty_param) = gen_param {
                        if let Some(call_ty) = call_generics.get(idx) {
                            type_subst.insert(ty_param.ident.to_string(), call_ty.clone());
                        }
                    }
                }

                // 3b. Substitutions-Map aus den Parametern und Argumenten aufbauen
                let mut subst = HashMap::new();
                for (idx, param) in target_rule.params.iter().enumerate() {
                    if let Some(arg) = args.get(idx) {
                        let arg_pattern = match arg {
                            Argument::Positional(p) => p.clone(),
                            Argument::Named(_, p) => p.clone(),
                        };
                        subst.insert(param.name.to_string(), arg_pattern);
                    }
                }

                // 4. AST der Regel klonen und Parameter ersetzen
                let mut inlined_variants = target_rule.variants.clone();
                for variant in &mut inlined_variants {
                    for step in &mut variant.pattern {
                        substitute_pattern(step, &subst);
                    }
                }

                // 5. Inlined Parser-Body direkt kompilieren (inkl. Typ-Generics in Return-Type)
                let mut ret_type = target_rule.return_type.clone();
                replace_type(&mut ret_type, &type_subst);

                let combined_lexical = is_lexical || target_rule.is_lexical || target_rule.name == "WS";
                let body = self.generate_variants_body(&inlined_variants, &ret_type, combined_lexical, true);
                let err_type = quote_spanned! {span=> ::winnow::error::InputError<::winnow_grammar::ParseInput<'a, S>> };
                
                return quote_spanned! {span=>
                    (|i: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                        let mut parser = (|i: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                            #body
                        });
                        ::winnow::Parser::parse_next(&mut parser, i)
                    })
                };
            }

            // --- Normaler Funktionsaufruf für NICHT-Template Regeln ---
            let fn_name = quote::format_ident!("parse_{}_inner", rule_name, span = span);
            if args.is_empty() {
                return quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| #fn_name(i)) };
            } else {
                let mut bindings = Vec::new();
                let mut arg_refs = Vec::new();
                
                for (idx, arg) in args.iter().enumerate() {
                    let arg_expr = self.generate_argument_expr(arg, is_lexical);
                    let ident = quote::format_ident!("arg_{}", idx, span = span);
                    bindings.push(quote_spanned! {span=> let mut #ident = #arg_expr; });
                    arg_refs.push(quote_spanned! {span=> &mut #ident });
                }

                return quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| {
                    #(#bindings)*
                    #fn_name(i, #(#arg_refs),*)
                }) };
            }
        }

        let err_type = quote_spanned! {span=> ::winnow::error::InputError<::winnow_grammar::ParseInput<'a, S>> };
        let input_type = quote_spanned! {span=> ::winnow_grammar::ParseInput<'a, S> };

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
                                '"' => '\"',
                                '0' => '\0',
                                _ => c // fallback
                             }
                        }),
                        ::winnow::token::none_of(['\\', '\''])
                    )),
                    '\''
                )
            },
            "any" => quote_spanned! {span=> ::winnow::token::any::<#input_type, #err_type> },
            "alpha1" => {
                quote_spanned! {span=> ::winnow::ascii::alpha1::<#input_type, #err_type> }
            }
            "digit1" => {
                quote_spanned! {span=> ::winnow::ascii::digit1::<#input_type, #err_type> }
            }
            "hex_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit0::<#input_type, #err_type> }
            }
            "hex_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::hex_digit1::<#input_type, #err_type> }
            }
            "oct_digit0" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit0::<#input_type, #err_type> }
            }
            "oct_digit1" => {
                quote_spanned! {span=> ::winnow::ascii::oct_digit1::<#input_type, #err_type> }
            }
            "binary_digit0" => quote_spanned! {span=>
                ::winnow::token::take_while(0.., |c| c == '0' || c == '1')
            },
            "binary_digit1" => quote_spanned! {span=>
                ::winnow::token::take_while(1.., |c| c == '0' || c == '1')
            },
            "space0" => {
                quote_spanned! {span=> ::winnow::ascii::space0::<#input_type, #err_type> }
            }
            "space1" => {
                quote_spanned! {span=> ::winnow::ascii::space1::<#input_type, #err_type> }
            }
            "multispace0" => {
                quote_spanned! {span=> ::winnow::ascii::multispace0::<#input_type, #err_type> }
            }
            "multispace1" => {
                quote_spanned! {span=> ::winnow::ascii::multispace1::<#input_type, #err_type> }
            }
            "line_ending" => {
                quote_spanned! {span=> ::winnow::ascii::line_ending::<#input_type, #err_type> }
            }
            "empty" => quote_spanned! {span=> ::winnow::combinator::empty },
            "eof" => quote_spanned! {span=> ::winnow::combinator::eof },

            "u8" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u8, #err_type> },
            "u16" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u16, #err_type> },
            "u32" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u32, #err_type> },
            "u64" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u64, #err_type> },
            "u128" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, u128, #err_type> },
            "usize" => quote_spanned! {span=> ::winnow::ascii::dec_uint::<#input_type, usize, #err_type> },
            "i8" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i8, #err_type> },
            "i16" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i16, #err_type> },
            "i32" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i32, #err_type> },
            "i64" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i64, #err_type> },
            "i128" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, i128, #err_type> },
            "isize" => quote_spanned! {span=> ::winnow::ascii::dec_int::<#input_type, isize, #err_type> },
            "f32" => quote_spanned! {span=> ::winnow::ascii::float::<#input_type, f32, #err_type> },
            "f64" => quote_spanned! {span=> ::winnow::ascii::float::<#input_type, f64, #err_type> },
            "bool" => quote_spanned! {span=>
                ::winnow::combinator::alt((
                    ::winnow::token::literal("true").map(|_| true),
                    ::winnow::token::literal("false").map(|_| false),
                ))
            },
            _ => {
                if args.is_empty() {
                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| ::winnow::Parser::parse_next(&mut #rule_path, i)) }
                } else {
                    let mut bindings = Vec::new();
                    let mut arg_refs = Vec::new();
                    
                    for (idx, arg) in args.iter().enumerate() {
                        let arg_expr = self.generate_argument_expr(arg, is_lexical);
                        let ident = quote::format_ident!("arg_{}", idx, span = span);
                        bindings.push(quote_spanned! {span=> let mut #ident = #arg_expr; });
                        arg_refs.push(quote_spanned! {span=> &mut #ident });
                    }

                    quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| {
                        #(#bindings)*
                        #rule_path(i, #(#arg_refs),*)
                    }) }
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
                rule_path, generics, args, ..
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
                parsers.push(quote_spanned! {span=> (|i: &mut ::winnow_grammar::ParseInput<'a, S>| WS(i)) });
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
