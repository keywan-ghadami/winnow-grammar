use super::pattern;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};
use syn::Result;
use syn_grammar_model::{analysis, model::*};

pub fn generate_rule(rule: &Rule, custom_keywords: &HashSet<String>) -> Result<TokenStream> {
    let name = &rule.name;
    let fn_name = format_ident!("parse_{}", name);
    let impl_name = format_ident!("parse_{}_impl", name);
    let ret_type = &rule.return_type;
    let attrs = &rule.attrs;

    // Filter attributes for the implementation function
    // Structural & Lint attributes must be on both.
    // API & Doc attributes should only be on the wrapper.
    let impl_attrs: Vec<&syn::Attribute> = attrs
        .iter()
        .filter(|a| {
            let p = a.path();
            p.is_ident("cfg")
                || p.is_ident("cfg_attr")
                || p.is_ident("allow")
                || p.is_ident("warn")
                || p.is_ident("deny")
                || p.is_ident("forbid")
        })
        .collect();

    // Default doc comment if none provided
    let default_doc = if attrs.iter().any(|a| a.path().is_ident("doc")) {
        quote!()
    } else {
        let msg = format!("Parser for rule `{}`.", name);
        quote!(#[doc = #msg])
    };

    let params: Vec<_> = rule
        .params
        .iter()
        .map(|(name, ty)| {
            quote! { , #name : #ty }
        })
        .collect();

    // Params for the impl call (forwarding arguments)
    let param_names: Vec<_> = rule
        .params
        .iter()
        .map(|(name, _)| {
            quote! { , #name }
        })
        .collect();

    let is_public = rule.is_pub || name == "main";
    let vis = if is_public { quote!(pub) } else { quote!() };

    // Check for direct left recursion
    let (recursive_refs, base_refs) = analysis::split_left_recursive(name, &rule.variants);

    let body = if recursive_refs.is_empty() {
        generate_variants_internal(&rule.variants, true, custom_keywords)?
    } else {
        if base_refs.is_empty() {
            return Err(syn::Error::new(
                name.span(),
                "Left-recursive rule requires at least one non-recursive base variant.",
            ));
        }

        let base_owned: Vec<RuleVariant> = base_refs.into_iter().cloned().collect();
        let recursive_owned: Vec<RuleVariant> = recursive_refs.into_iter().cloned().collect();

        let base_logic = generate_variants_internal(&base_owned, true, custom_keywords)?;
        let loop_logic = generate_recursive_loop_body(&recursive_owned, custom_keywords)?;

        quote! {
            let mut lhs = {
                let base_parser = |input: ParseStream, ctx: &mut rt::ParseContext| -> Result<#ret_type> {
                    #base_logic
                };
                base_parser(input, ctx)?
            };
            loop {
                #loop_logic
                break;
            }
            Ok(lhs)
        }
    };

    Ok(quote! {
        #(#attrs)*
        #default_doc
        #vis fn #fn_name(input: ParseStream #(#params)*) -> Result<#ret_type> {
            let mut ctx = rt::ParseContext::new();
            match #impl_name(input, &mut ctx #(#param_names)*) {
                Ok(val) => Ok(val),
                Err(e) => {
                    if let Some(best) = ctx.take_best_error() {
                        Err(best)
                    } else {
                        Err(e)
                    }
                }
            }
        }

        #[doc(hidden)]
        #(#impl_attrs)*
        pub fn #impl_name(input: ParseStream, ctx: &mut rt::ParseContext #(#params)*) -> Result<#ret_type> {
            ctx.enter_rule(stringify!(#name));
            let res = (|| -> syn::Result<#ret_type> {
                #body
            })();
            ctx.exit_rule();
            res
        }
    })
}

fn generate_recursive_loop_body(
    variants: &[RuleVariant],
    kws: &HashSet<String>,
) -> Result<TokenStream> {
    let arms = variants.iter().map(|variant| {
        let tail_pattern = &variant.pattern[1..];

        let lhs_binding = match &variant.pattern[0] {
            ModelPattern::RuleCall { binding: Some(b), .. } => Some(b),
            _ => None
        };

        let bind_stmt = if let Some(b) = lhs_binding {
            quote! { let #b = lhs.clone(); }
        } else {
            quote! {}
        };

        let logic = pattern::generate_sequence(tail_pattern, &variant.action, kws)?;

        let peek_token_obj = tail_pattern.first()
            .and_then(|f| analysis::get_simple_peek(f, kws).ok().flatten());

        match peek_token_obj {
            Some(token_code) => {
                Ok(quote! {
                    if input.peek(#token_code) {
                        let _start_cursor = input.cursor();
                        // Pass ctx to attempt
                        if let Some(new_val) = rt::attempt(input, ctx, |input, ctx| {
                            #bind_stmt
                            #logic
                        })? {
                            if _start_cursor == input.cursor() {
                                return Err(input.error("Left-recursive rule matched empty string (infinite loop detected)"));
                            }
                            lhs = new_val;
                            continue;
                        }
                    }
                })
            },
            None => {
                Ok(quote! {
                    let _start_cursor = input.cursor();
                    // Pass ctx to attempt
                    if let Some(new_val) = rt::attempt(input, ctx, |input, ctx| {
                        #bind_stmt
                        #logic
                    })? {
                        if _start_cursor == input.cursor() {
                            return Err(input.error("Left-recursive rule matched empty string (infinite loop detected)"));
                        }
                        lhs = new_val;
                        continue;
                    }
                })
            }
        }
    }).collect::<Result<Vec<_>>>()?;

    Ok(quote! { #(#arms)* })
}

pub fn generate_variants_internal(
    variants: &[RuleVariant],
    is_top_level: bool,
    _custom_keywords: &HashSet<String>,
) -> Result<TokenStream> {
    if variants.is_empty() {
        return Ok(quote! { Err(input.error("No variants defined")) });
    }

    let mut token_counts = HashMap::new();
    for v in variants {
        let is_nullable = v.pattern.first().is_none_or(analysis::is_nullable);
        if !is_nullable {
            if let Some(token_str) = analysis::get_peek_token_string(&v.pattern) {
                *token_counts.entry(token_str).or_insert(0) += 1;
            }
        }
    }

    let arms = variants
        .iter()
        .map(|variant| {
            let cut_info = analysis::find_cut(&variant.pattern);
            let first_pat = variant.pattern.first();
            let is_nullable = first_pat.is_none_or(analysis::is_nullable);

            let peek_token_obj = if !is_nullable {
                first_pat.and_then(|f| {
                    analysis::get_simple_peek(f, _custom_keywords)
                        .ok()
                        .flatten()
                })
            } else {
                None
            };

            let peek_str = if !is_nullable {
                analysis::get_peek_token_string(&variant.pattern)
            } else {
                None
            };

            let is_unique = if let (_, Some(token_key)) = (&peek_token_obj, &peek_str) {
                token_counts
                    .get(token_key)
                    .map(|c| *c == 1)
                    .unwrap_or(false)
            } else {
                false
            };

            if let Some(cut) = cut_info {
                let pre_cut = cut.pre_cut;
                let post_cut = cut.post_cut;

                let pre_bindings = analysis::collect_bindings(pre_cut);
                let pre_logic = pattern::generate_sequence_steps(pre_cut, _custom_keywords)?;
                let post_logic = pattern::generate_sequence_steps(post_cut, _custom_keywords)?;
                let action = &variant.action;

                let logic_block = if is_unique {
                    quote! {
                        {
                            let mut run = || -> syn::Result<_> {
                                #pre_logic
                                #post_logic
                                Ok({ #action })
                            };
                            match run() {
                                Ok(v) => return Ok(v),
                                Err(e) => {
                                    ctx.set_fatal(true); // Use ctx
                                    return Err(e);
                                }
                            }
                        }
                    }
                } else {
                    quote! {
                        // Pass ctx to attempt
                        let pre_result = rt::attempt(input, ctx, |input, ctx| {
                            #pre_logic
                            Ok(( #(#pre_bindings),* ))
                        })?;

                        if let Some(( #(#pre_bindings),* )) = pre_result {
                            let mut post_run = || -> syn::Result<_> {
                                #post_logic
                                Ok({ #action })
                            };
                            match post_run() {
                                Ok(v) => return Ok(v),
                                Err(e) => {
                                    ctx.set_fatal(true); // Use ctx
                                    return Err(e);
                                }
                            }
                        }
                    }
                };

                if let Some(token_code) = peek_token_obj {
                    Ok(quote! {
                        if input.peek(#token_code) {
                            #logic_block
                        }
                    })
                } else {
                    Ok(logic_block)
                }
            } else {
                let logic = pattern::generate_sequence(
                    &variant.pattern,
                    &variant.action,
                    _custom_keywords,
                )?;

                if is_unique {
                    let token_code = peek_token_obj.as_ref().unwrap();
                    Ok(quote! {
                        if input.peek(#token_code) {
                            let mut run = || -> syn::Result<_> {
                                #logic
                            };
                            match run() {
                                Ok(v) => return Ok(v),
                                Err(e) => {
                                    ctx.set_fatal(true); // Use ctx
                                    return Err(e);
                                }
                            }
                        }
                    })
                } else if let Some(token_code) = peek_token_obj {
                    Ok(quote! {
                        if input.peek(#token_code) {
                            // Pass ctx to attempt
                            if let Some(res) = rt::attempt(input, ctx, |input, ctx| { #logic })? {
                                return Ok(res);
                            }
                        }
                    })
                } else {
                    Ok(quote! {
                        // Pass ctx to attempt
                        if let Some(res) = rt::attempt(input, ctx, |input, ctx| { #logic })? {
                            return Ok(res);
                        }
                    })
                }
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let error_msg = if is_top_level {
        "No matching rule variant found"
    } else {
        "No matching variant in group"
    };

    Ok(quote! {
        #(#arms)*

        if let Some(best_err) = ctx.take_best_error() { // Use ctx
            Err(best_err)
        } else {
            Err(input.error(#error_msg))
        }
    })
}
