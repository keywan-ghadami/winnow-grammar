use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::Result;
use syn_grammar_model::{analysis, model::*};

pub fn generate_sequence(
    patterns: &[ModelPattern],
    action: &TokenStream,
    kws: &HashSet<String>,
) -> Result<TokenStream> {
    let steps = generate_sequence_steps(patterns, kws)?;
    Ok(quote! { { #steps Ok({ #action }) } })
}

pub fn generate_sequence_steps(
    patterns: &[ModelPattern],
    kws: &HashSet<String>,
) -> Result<TokenStream> {
    let steps = patterns
        .iter()
        .map(|p| generate_pattern_step(p, kws))
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! { #(#steps)* })
}

fn generate_pattern_step(pattern: &ModelPattern, kws: &HashSet<String>) -> Result<TokenStream> {
    let span = pattern.span();

    match pattern {
        ModelPattern::Cut(_) => Ok(quote!()),
        ModelPattern::Lit(lit) => {
            let token_types = analysis::resolve_token_types(lit, kws)?;

            if token_types.len() <= 1 {
                let parses = token_types.iter().map(|ty| {
                    quote! { let _ = input.parse::<#ty>()?; }
                });
                Ok(quote! { #(#parses)* })
            } else {
                let mut steps = Vec::new();
                let mut checks = Vec::new();

                for (i, ty) in token_types.iter().enumerate() {
                    let var = format_ident!("_t{}", i);
                    steps.push(quote! {
                        let #var = input.parse::<#ty>()?;
                    });

                    if i > 0 {
                        let prev = format_ident!("_t{}", i - 1);
                        let err_msg =
                            format!("expected '{}', found space between tokens", lit.value());
                        checks.push(quote! {
                            if #prev.span().end() != #var.span().start() {
                                return Err(syn::Error::new(
                                    #var.span(),
                                    #err_msg
                                ));
                            }
                        });
                    }
                }

                Ok(quote! {
                    {
                        #(#steps)*
                        #(#checks)*
                    }
                })
            }
        }
        ModelPattern::RuleCall {
            binding,
            rule_name,
            args,
        } => {
            let func_call = generate_rule_call_expr(rule_name, args);
            Ok(if let Some(bind) = binding {
                quote! { let #bind = #func_call; }
            } else {
                quote! { let _ = #func_call; }
            })
        }

        ModelPattern::Repeat(inner, _) => {
            let bindings = analysis::collect_bindings(std::slice::from_ref(inner));

            if !bindings.is_empty() {
                // Use temporary names for vectors to avoid shadowing by inner bindings
                let vec_names: Vec<_> = bindings
                    .iter()
                    .map(|b| format_ident!("_vec_{}", b))
                    .collect();

                let init_vecs: Vec<_> = vec_names
                    .iter()
                    .map(|v| quote!(let mut #v = Vec::new();))
                    .collect();
                let push_vecs: Vec<_> = vec_names
                    .iter()
                    .zip(bindings.iter())
                    .map(|(v, b)| quote!(#v.push(#b);))
                    .collect();
                let finalize_vecs: Vec<_> = bindings
                    .iter()
                    .zip(vec_names.iter())
                    .map(|(b, v)| quote!(let #b = #v;))
                    .collect();

                let inner_logic = generate_pattern_step(inner, kws)?;

                let peek_check = if let Some(peek) = analysis::get_simple_peek(inner, kws)? {
                    quote!(input.peek(#peek))
                } else {
                    quote!(true)
                };

                if analysis::get_simple_peek(inner, kws)?.is_some() {
                    Ok(quote! {
                       #(#init_vecs)*
                       while #peek_check {
                           {
                               #inner_logic
                               #(#push_vecs)*
                           }
                       }
                       #(#finalize_vecs)*
                    })
                } else {
                    let return_tuple = quote!(( #(#bindings),* ));
                    let tuple_pat = quote!(( #(#bindings),* ));

                    Ok(quote! {
                       #(#init_vecs)*
                       // Pass ctx to attempt
                       while let Some(vals) = rt::attempt(input, ctx, |input, ctx| {
                           #inner_logic
                           Ok(#return_tuple)
                       })? {
                           let #tuple_pat = vals;
                           #(#push_vecs)*
                       }
                       #(#finalize_vecs)*
                    })
                }
            } else {
                let inner_logic = generate_pattern_step(inner, kws)?;
                Ok(quote! {
                    // Pass ctx to attempt
                    while let Some(_) = rt::attempt(input, ctx, |input, ctx| { #inner_logic Ok(()) })? {}
                })
            }
        }

        ModelPattern::Plus(inner, _) => {
            let bindings = analysis::collect_bindings(std::slice::from_ref(inner));

            if !bindings.is_empty() {
                // Use temporary names for vectors to avoid shadowing by inner bindings
                let vec_names: Vec<_> = bindings
                    .iter()
                    .map(|b| format_ident!("_vec_{}", b))
                    .collect();

                let init_vecs: Vec<_> = vec_names
                    .iter()
                    .map(|v| quote!(let mut #v = Vec::new();))
                    .collect();
                let push_vecs: Vec<_> = vec_names
                    .iter()
                    .zip(bindings.iter())
                    .map(|(v, b)| quote!(#v.push(#b);))
                    .collect();
                let finalize_vecs: Vec<_> = bindings
                    .iter()
                    .zip(vec_names.iter())
                    .map(|(b, v)| quote!(let #b = #v;))
                    .collect();

                let inner_logic = generate_pattern_step(inner, kws)?;

                let peek_check = if let Some(peek) = analysis::get_simple_peek(inner, kws)? {
                    quote!(input.peek(#peek))
                } else {
                    quote!(true)
                };

                if analysis::get_simple_peek(inner, kws)?.is_some() {
                    Ok(quote! {
                       #(#init_vecs)*
                       {
                           #inner_logic
                           #(#push_vecs)*
                       }
                       while #peek_check {
                           {
                               #inner_logic
                               #(#push_vecs)*
                           }
                       }
                       #(#finalize_vecs)*
                    })
                } else {
                    let return_tuple = quote!(( #(#bindings),* ));
                    let tuple_pat = quote!(( #(#bindings),* ));

                    Ok(quote! {
                       #(#init_vecs)*
                       {
                           #inner_logic
                           #(#push_vecs)*
                       }
                       // Pass ctx to attempt
                       while let Some(vals) = rt::attempt(input, ctx, |input, ctx| {
                           #inner_logic
                           Ok(#return_tuple)
                       })? {
                           let #tuple_pat = vals;
                           #(#push_vecs)*
                       }
                       #(#finalize_vecs)*
                    })
                }
            } else {
                let inner_logic = generate_pattern_step(inner, kws)?;
                Ok(quote! {
                    #inner_logic
                    // Pass ctx to attempt
                    while let Some(_) = rt::attempt(input, ctx, |input, ctx| { #inner_logic Ok(()) })? {}
                })
            }
        }

        ModelPattern::Optional(inner, _) => {
            let inner_logic = generate_pattern_step(inner, kws)?;
            let peek_opt = analysis::get_simple_peek(inner, kws)?;
            let is_nullable = analysis::is_nullable(inner);

            if let (Some(peek), false) = (peek_opt, is_nullable) {
                Ok(quote! {
                    if input.peek(#peek) {
                        // Pass ctx to attempt
                        let _ = rt::attempt(input, ctx, |input, ctx| { #inner_logic Ok(()) })?;
                    }
                })
            } else {
                Ok(quote! {
                    // Pass ctx to attempt
                    let _ = rt::attempt(input, ctx, |input, ctx| { #inner_logic Ok(()) })?;
                })
            }
        }
        ModelPattern::Group(alts, _) => {
            use super::rule::generate_variants_internal;
            let temp_variants = alts
                .iter()
                .map(|pat_seq| RuleVariant {
                    pattern: pat_seq.clone(),
                    action: quote!({}),
                })
                .collect::<Vec<_>>();
            let variant_logic = generate_variants_internal(&temp_variants, false, kws)?;
            Ok(quote! { { #variant_logic }?; })
        }

        ModelPattern::Bracketed(s, _)
        | ModelPattern::Braced(s, _)
        | ModelPattern::Parenthesized(s, _) => {
            let macro_name = match pattern {
                ModelPattern::Bracketed(_, _) => quote!(bracketed),
                ModelPattern::Braced(_, _) => quote!(braced),
                _ => quote!(parenthesized),
            };

            let inner_logic = generate_sequence_steps(s, kws)?;
            let bindings = analysis::collect_bindings(s);

            if bindings.is_empty() {
                Ok(quote! { {
                    let content;
                    let _ = syn::#macro_name!(content in input);
                    let input = &content;
                    #inner_logic
                }})
            } else if bindings.len() == 1 {
                let bind = &bindings[0];
                Ok(quote! {
                    let #bind = {
                        let content;
                        let _ = syn::#macro_name!(content in input);
                        let input = &content;
                        #inner_logic
                        #bind
                    };
                })
            } else {
                Ok(quote! {
                    let (#(#bindings),*) = {
                        let content;
                        let _ = syn::#macro_name!(content in input);
                        let input = &content;
                        #inner_logic
                        (#(#bindings),*)
                    };
                })
            }
        }

        ModelPattern::SpanBinding(inner, span_var, _) => {
            let (inner_pat, binding_name) = match &**inner {
                ModelPattern::RuleCall {
                    binding,
                    rule_name,
                    args,
                } => {
                    if let Some(b) = binding {
                        (inner.clone(), b.clone())
                    } else {
                        let temp = format_ident!("_val_{}", span_var);
                        let new_inner = ModelPattern::RuleCall {
                            binding: Some(temp.clone()),
                            rule_name: rule_name.clone(),
                            args: args.clone(),
                        };
                        (Box::new(new_inner), temp)
                    }
                }
                ModelPattern::Recover {
                    binding,
                    body,
                    sync,
                    span,
                } => {
                    if let Some(b) = binding {
                        (inner.clone(), b.clone())
                    } else {
                        let temp = format_ident!("_val_{}", span_var);
                        let new_inner = ModelPattern::Recover {
                            binding: Some(temp.clone()),
                            body: body.clone(),
                            sync: sync.clone(),
                            span: *span,
                        };
                        (Box::new(new_inner), temp)
                    }
                }
                _ => {
                    return Err(syn::Error::new(
                        span_var.span(),
                        "Span binding (@) is currently only supported on rule calls and recover() blocks.",
                    ));
                }
            };

            let inner_code = generate_pattern_step(&inner_pat, kws)?;

            Ok(quote! {
                #inner_code
                let #span_var = syn::spanned::Spanned::span(&#binding_name);
            })
        }

        ModelPattern::Recover {
            binding,
            body,
            sync,
            ..
        } => {
            let effective_body = if let Some(bind) = binding {
                match &**body {
                    ModelPattern::RuleCall { binding: None, rule_name, args } => {
                        Box::new(ModelPattern::RuleCall {
                            binding: Some(bind.clone()),
                            rule_name: rule_name.clone(),
                            args: args.clone()
                        })
                    },
                    _ => return Err(syn::Error::new(span, "Binding on recover(...) is only supported if the body is a direct rule call."))
                }
            } else {
                body.clone()
            };

            let inner_logic = generate_pattern_step(&effective_body, kws)?;
            let sync_peek = analysis::get_simple_peek(sync, kws)?.ok_or_else(|| {
                syn::Error::new(
                    sync.span(),
                    "Sync pattern in recover(...) must have a simple start token.",
                )
            })?;

            let bindings = analysis::collect_bindings(std::slice::from_ref(&effective_body));

            if bindings.is_empty() {
                Ok(quote! {
                    // Pass ctx to attempt_recover
                    if rt::attempt_recover(input, ctx, |input, ctx| { #inner_logic Ok(()) })?.is_none() {
                        rt::skip_until(input, |i| i.peek(#sync_peek))?;
                    }
                })
            } else {
                let none_exprs = bindings.iter().map(|_| quote!(Option::<_>::None));

                Ok(quote! {
                    // Pass ctx to attempt_recover
                    let (#(#bindings),*) = match rt::attempt_recover(input, ctx, |input, ctx| {
                        #inner_logic
                        Ok((#(#bindings),*))
                    })? {
                        Some(vals) => {
                            let (#(#bindings),*) = vals;
                            (#(Some(#bindings)),*)
                        },
                        None => {
                            rt::skip_until(input, |i| i.peek(#sync_peek))?;
                            (#(#none_exprs),*)
                        }
                    };
                })
            }
        }
    }
}

fn generate_rule_call_expr(rule_name: &syn::Ident, args: &[syn::Lit]) -> TokenStream {
    // Call the _impl version and pass ctx
    let f = format_ident!("parse_{}_impl", rule_name);
    if args.is_empty() {
        quote!(#f(input, ctx)?)
    } else {
        quote!(#f(input, ctx, #(#args),*)?)
    }
}
