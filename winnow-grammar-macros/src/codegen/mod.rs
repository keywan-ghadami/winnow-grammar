pub mod expr;
pub mod rule;
pub mod variants;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote_spanned};
use std::collections::HashSet;
use winnow_grammar_model::model::GrammarDefinition;

/// Ist die Regel eine Vorlage, die nicht als eigene Funktion erzeugt, sondern
/// an jeder Aufrufstelle eingesetzt wird?
///
/// Das ist sie, sobald sie einen *Parser*-Parameter hat - in einer der beiden
/// Schreibweisen:
///
/// * `list<T>(item: Rule<T>)` - der Parameter ist als Regel deklariert und sein
///   Ergebnistyp an `T` gebunden;
/// * `list<T>(item)` - ohne Typ. `T` wird dann aus dem Argument abgeleitet
///   (siehe `Codegen::leite_typ_ab`).
///
/// Vorher galt nur die erste Form als Vorlage. Die zweite lief in den Pfad fuer
/// Laufzeitparameter, dessen innere Funktion den Parameter `item_wrapper`
/// nennt, waehrend der Rumpf `item` sagt - daher ``cannot find value `item` ``.
pub(crate) fn ist_vorlage(rule: &winnow_grammar_model::model::Rule) -> bool {
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
    let mut codegen = Codegen::new(&grammar);
    codegen.generate()
}

pub struct Codegen<'a> {
    grammar: &'a GrammarDefinition,
    pub user_rules: HashSet<String>,
    pub input_ident: syn::Ident,
}

impl<'a> Codegen<'a> {
    pub fn new(grammar: &'a GrammarDefinition) -> Self {
        let user_rules = grammar.rules.iter().map(|r| r.name.to_string()).collect();
        Self {
            grammar,
            user_rules,
            input_ident: format_ident!("input", span = Span::call_site()),
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
                ) -> ::winnow::Result<(), ::winnow::error::ErrMode<::winnow::error::ContextError>> {
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
