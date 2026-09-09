pub mod expr;
pub mod rule;
pub mod variants;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use winnow_grammar_model::frame::Frames;
use winnow_grammar_model::model::GrammarDefinition;
use winnow_grammar_model::ParsedGrammar;

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

pub fn generate_rust(parsed: ParsedGrammar) -> syn::Result<TokenStream> {
    // Validation ran the frame check once and hands its result over; the
    // generator does not run it again.
    let ParsedGrammar {
        grammar, frames, ..
    } = parsed;
    let mut codegen = Codegen::new(&grammar, frames);
    codegen.generate()
}

pub struct Codegen<'a> {
    grammar: &'a GrammarDefinition,
    pub user_rules: HashSet<String>,
    pub input_ident: syn::Ident,
    /// What `frame::check` established: frame rules and their boundaries,
    /// which rules are a `par_fold` over which frame, and what `frame_end`
    /// stands for in each rule a frame reaches.
    pub frames: Frames,
    /// What `frame_end` stands for in the rule being generated - set by
    /// `generate_rule`, read where a `frame_end` is met. It resolves a name
    /// the grammar wrote; it never changes a pattern the grammar did not.
    pub current_boundary: RefCell<Option<String>>,
    /// Rules that match nothing but literals - a terminator may be one, and
    /// then it is scanned for like the literal it is. The frame check reads
    /// the same map, so the two cannot disagree about what a terminator is.
    pub literal_rules: HashMap<String, Vec<String>>,
}

impl Codegen<'_> {
    /// The extra bound `state T;` puts on a rule's state parameter, empty for
    /// a grammar that declares none. A bound rather than a substitution, so
    /// rules stay generic and one state can serve two grammars - ADR 20.
    pub fn state_bound(&self) -> TokenStream {
        let state = match &self.grammar.state {
            Some(ty) => quote! { + ::winnow_grammar::StateOf<#ty> },
            None => quote! {},
        };
        // `interner I;` rides on the same `S`, for the same reason - ADR 22.
        let interner = match &self.grammar.interner {
            Some(ty) => quote! { + ::winnow_grammar::InternerOf<#ty> },
            None => quote! {},
        };
        quote! { #state #interner }
    }
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
            literal_rules: winnow_grammar_model::analysis::literal_rules(grammar),
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

        // `state T;`: a grammar-local extension trait, so that an action
        // writes `_state.user()` and gets a `&mut T` with no turbofish and no
        // second live borrow of the context - ADR 20.
        let interner_accessor = match &self.grammar.interner {
            Some(ty) => quote_spanned! {Span::call_site()=>
                trait __DeclaredInterner { fn declared_interner(&mut self) -> &mut #ty; }
                impl<S: ::winnow_grammar::InternerOf<#ty>> __DeclaredInterner
                    for ::winnow_grammar::ParseContext<S>
                {
                    #[inline]
                    fn declared_interner(&mut self) -> &mut #ty {
                        ::winnow_grammar::InternerOf::<#ty>::interner(&mut self.user_state)
                    }
                }
            },
            None => quote! {},
        };

        let state_accessor = match &self.grammar.state {
            Some(ty) => quote_spanned! {Span::call_site()=>
                trait __UserState { fn user(&mut self) -> &mut #ty; }
                impl<S: ::winnow_grammar::StateOf<#ty>> __UserState
                    for ::winnow_grammar::ParseContext<S>
                {
                    #[inline]
                    fn user(&mut self) -> &mut #ty {
                        ::winnow_grammar::StateOf::state(&mut self.user_state)
                    }
                }
            },
            None => quote! {},
        };

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
                fn WS<'a, S: std::fmt::Debug + Clone, E: ::winnow_grammar::rt::RtError<'a, S>>(
                    #input: &mut ::winnow_grammar::ParseInput<'a, S>,
                ) -> ::winnow::Result<(), ::winnow::error::ErrMode<E>> {
                    use ::winnow::Parser;
                    // The default whitespace skip runs between every two
                    // tokens, so it is the hottest scan in a generated parser
                    // - and a word at a time (`src/ascii.rs`), not a
                    // character at a time.
                    ::winnow_grammar::rt::class::<S, E>(
                        ::winnow_grammar::ascii::AsciiClass::MULTISPACE,
                        0,
                    )
                    .parse_next(#input)
                    .map(|_| ())
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

                #state_accessor
                #interner_accessor

                use ::winnow::prelude::*;
                use ::winnow::token::literal;
                use ::winnow::combinator::{alt, repeat, opt, delimited, preceded};

                #ws_parser

                #(#rules)*
            }
        })
    }
}
