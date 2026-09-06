pub mod expr;
pub mod rule;
pub mod variants;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote_spanned};
use std::cell::RefCell;
use std::collections::HashSet;
use winnow_grammar_model::frame::Frames;
use winnow_grammar_model::model::GrammarDefinition;

/// Is the rule a template that is not generated as a function of its own but
/// inlined at every call site?
///
/// It is, as soon as it has a *parser* parameter - in either of the two
/// notations:
///
/// * `list<T>(item: Rule<T>)` - the parameter is declared as a rule and its
///   result type bound to `T`;
/// * `list<T>(item)` - without a type. `T` is then inferred from the argument
///   (see `Codegen::infer_type`).
///
/// Previously only the first form counted as a template. The second went down
/// the path for runtime parameters, whose inner function names the parameter
/// `item_wrapper` while the body says `item` - hence ``cannot find value `item` ``.
pub(crate) fn is_template(rule: &winnow_grammar_model::model::Rule) -> bool {
    rule.params.iter().any(|p| match &p.ty {
        None => true,
        Some(syn::Type::Path(type_path)) => type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Rule"),
        Some(_) => false,
    })
}

pub fn generate_rust(grammar: GrammarDefinition) -> syn::Result<TokenStream> {
    // Validation has already run this check and reported its errors; running
    // it again here is how the generator learns which skips to bound.
    use winnow_grammar_model::Backend;
    let builtins: HashSet<String> = crate::WinnowBackend::get_builtins()
        .iter()
        .map(|b| b.name.to_string())
        .collect();
    let frames = winnow_grammar_model::frame::check(&grammar, &builtins)?;
    let mut codegen = Codegen::new(&grammar, frames);
    codegen.generate()
}

pub struct Codegen<'a> {
    grammar: &'a GrammarDefinition,
    pub user_rules: HashSet<String>,
    pub input_ident: syn::Ident,
    /// What `frame::check` established: frame rules, their boundaries, and
    /// which rules' skips must stop at one.
    pub frames: Frames,
    /// The boundary bounding skips in the rule currently being generated -
    /// set by `generate_rule`, read by `generate_skip_to`. A template inlined
    /// at a call site is generated under the caller's boundary, which is the
    /// right one: it is that caller's format the template is part of there.
    pub current_boundary: RefCell<Option<String>>,
}

impl<'a> Codegen<'a> {
    pub fn new(grammar: &'a GrammarDefinition, frames: Frames) -> Self {
        let user_rules = grammar.rules.iter().map(|r| r.name.to_string()).collect();
        Self {
            grammar,
            user_rules,
            input_ident: format_ident!("input", span = Span::call_site()),
            frames,
            current_boundary: RefCell::new(None),
        }
    }

    pub fn generate(&mut self) -> syn::Result<TokenStream> {
        let grammar_name = &self.grammar.name;
        let span = Span::mixed_site();
        let use_statements = &self.grammar.uses;
        let input = &self.input_ident;

        let has_user_ws = self.user_rules.contains("WS");

        let rules = self.grammar.rules.iter().map(|r| self.generate_rule(r));

        let use_super = quote_spanned! {Span::call_site()=> use super::*; };

        // If user defined WS, we alias WS to parse_WS_inner so that internal usage (and wrappers) call the inner parser directly.
        let ws_parser = if has_user_ws {
            quote_spanned! {span=>
                #[allow(unused_imports)]
                use parse_WS_inner as WS;
            }
        } else {
            quote_spanned! {span=>
                // Whitespace handling (similar to syn)
                #[allow(dead_code)]
                fn WS<'a, S: std::fmt::Debug + Clone>(
                    #input: &mut ::winnow_grammar::ParseInput<'a, S>,
                ) -> ::winnow::Result<(), ::winnow::error::ErrMode<::winnow_grammar::ParseError>> {
                    use ::winnow::Parser;
                    ::winnow::ascii::multispace0.parse_next(#input).map(|_| ())
                }
            }
        };

        Ok(quote_spanned! {span=>
            #[allow(non_snake_case)]
            pub mod #grammar_name {
                #![allow(unused_imports)]
                #![allow(dead_code)]

                // Import types from parent module (e.g. AST structs)
                #use_super

                // User-defined use statements
                #(#use_statements)*

                use ::winnow::prelude::*;
                use ::winnow::token::literal;
                use ::winnow::combinator::{alt, repeat, opt, delimited, preceded};

                #ws_parser

                #(#rules)*
            }
        })
    }
}
