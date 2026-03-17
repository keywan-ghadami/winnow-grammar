use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn_grammar_model::{
    analysis,
    model::{Rule, RuleVariant},
};
use syn;

impl<'a> Codegen<'a> {
    pub fn generate_rule(&self, rule: &Rule) -> TokenStream {
        let rule_name = &rule.name;
        let rule_name_str = rule_name.to_string();
        let is_ws_rule = rule_name_str == "WS";
        let span = Span::mixed_site();
        let fn_name = format_ident!("parse_{}", rule_name, span = span);
        let inner_fn_name = format_ident!("parse_{}_inner", rule_name, span = span);
        let ret_type = &rule.return_type;
        let input = &self.input_ident;
        let err_type = quote_spanned! { span=> ::winnow::error::InputError<::winnow_grammar::ParseInput<'a, S>> };

        let mut params_tokens = Vec::new();
        let mut inner_params_tokens = Vec::new();
        let mut arg_names = Vec::new();
        let mut extra_generics = Vec::<syn::Ident>::new();

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
                                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                                    let inner_args = &args.args;
                                    actual_ty = quote! { impl ::winnow::Parser<::winnow_grammar::ParseInput<'a, S>, #inner_args, #err_type> };
                                }
                            }
                        }
                    }
                    params_tokens.push(quote! { mut #name: #actual_ty });
                    if is_parser {
                        inner_params_tokens.push(quote! { #name: &mut #actual_ty });
                        arg_names.push(quote! { &mut #name });
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
                    inner_params_tokens.push(quote! { #name: &mut #actual_ty });
                    arg_names.push(quote! { &mut #name });
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
                fn WS<'a, S: std::fmt::Debug + Clone>(_: &mut ::winnow_grammar::ParseInput<'a, S>) -> ::winnow::Result<(), #err_type> {
                    Ok(())
                }
            }
        } else {
            quote! {}
        };

        let inner_fn = quote_spanned! {span=>
            #[allow(dead_code)]
            fn #inner_fn_name<#all_generics>(#input: &mut ::winnow_grammar::ParseInput<'a, S>, #(#inner_params_tokens),*) -> ::winnow::Result<#ret_type, #err_type>
            where
                #where_preds
            {
                use ::winnow::Parser;

                #ws_shadow

                let mut parser = (|#input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                    use ::winnow::prelude::*;
                    #body
                })
                .context(::winnow::error::StrContext::Label(#rule_name_str));

                #[cfg(feature = "trace")]
                {
                    ::winnow::combinator::trace(#rule_name_str, parser).parse_next(#input)
                }

                #[cfg(not(feature = "trace"))]
                {
                    parser.parse_next(#input)
                }
            }
        };

        let mut outer_generics = quote! {'a, S: std::fmt::Debug + Clone };
        if !extra_generics.is_empty() {
            outer_generics.extend(quote! {, #(#extra_generics),* });
        }
        if !gen_params.is_empty() {
            outer_generics.extend(quote! {, #gen_params});
        }


        let outer_fn_body = quote! {
            move |input: &mut ::winnow_grammar::ParseInput<'a, S>| -> ::winnow::Result<#ret_type, #err_type> {
                // Public API wrapper to handle whitespace and EOF
                let _ = WS(input)?;

                // Call inner rule
                let result = #inner_fn_name(input, #(#arg_names),*)?;

                let _ = WS(input)?;

                // EOF check
                ::winnow::combinator::eof.parse_next(input)?;

                Ok(result)
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

        quote! {
            #inner_fn
            #outer_fn
        }
    }
}