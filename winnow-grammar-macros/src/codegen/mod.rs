pub mod edge;
pub mod expr;
pub mod fixed;
pub mod rule;
pub mod stream;
pub mod variants;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use winnow_grammar_model::ParsedGrammar;
use winnow_grammar_model::frame::Frames;
use winnow_grammar_model::model::{GrammarDefinition, InputKind};
use winnow_grammar_model::stream::Streams;

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
        grammar,
        frames,
        streams,
        ..
    } = parsed;
    let mut codegen = Codegen::new(&grammar, frames, streams);
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
    /// The bytes each alternative can begin with, where that is known - the
    /// fast pass's guard on a rule's alternatives (`rt::first_byte`).
    pub first_bytes: winnow_grammar_model::first_bytes::FirstBytes<'a>,
    /// Numbers the cut scopes of one grammar, for their depth bindings.
    pub cut_counter: std::cell::Cell<usize>,
    /// `#[stream]` rules and their paths (ADR 27 §3).
    pub streams: Streams,
    /// Whether the parsers being generated now are wrapped in `rt::edge`.
    /// In a grammar with a `#[stream]` rule it is set while the tracked
    /// copies are generated and clear for the plain ones (`dual`); where
    /// `WINNOW_GRAMMAR_EDGES` is set when the macro expands it is set for
    /// the one copy there is - which is how the whole test suite is run once
    /// with the wrapping on, to show that it changes no result.
    pub streaming: std::cell::Cell<bool>,
    /// Whether every rule's parser is generated twice (ADR 27 §3, *Per-mode
    /// copies*): untracked for the plain entry points, and tracked under
    /// `__edge_…` names for the stream readers and everything they reach.
    /// In a grammar with a `#[stream]` rule, unless `WINNOW_GRAMMAR_EDGES`
    /// asks for one tracked copy of everything.
    pub dual: bool,
    /// The rules the tracked copy of a `dual` grammar holds: the stream
    /// rules, what they reach, and the grammar's own `WS`. A rule no stream
    /// reaches is only ever parsed in memory and gets no tracked copy.
    pub tracked_rules: std::collections::HashSet<String>,
    /// How far past where it stops, or where it began when it fails, any
    /// parser of this grammar looks - `rt::edge`'s `k`.
    pub edge_k: usize,
    /// Whether an item of a stream may record a failure past its end, and
    /// the parsers note how far failures reach (`edge::reaches`).
    pub reach: bool,
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
        // `interner I;` rides on the same `__WgS`, for the same reason - ADR 22.
        let interner = match &self.grammar.interner {
            Some(ty) => quote! { + ::winnow_grammar::InternerOf<#ty> },
            None => quote! {},
        };
        quote! { #state #interner }
    }

    /// The collection a bound repetition collects into, written without its
    /// element type: `collect L;`'s `L`, or `::std::vec::Vec`. By its full
    /// path, so that a crate with a `Vec` of its own in scope - `use super::*`
    /// brings it into the grammar's module - does not change what it means.
    pub fn collection(&self) -> TokenStream {
        match &self.grammar.collect {
            Some(path) => quote! { #path },
            None => quote! { ::std::vec::Vec },
        }
    }

    /// The whitespace skip the parsers being generated call: `WS`, or its
    /// tracked copy `__edge_WS` while the tracked copies are generated.
    pub fn ws(&self) -> syn::Ident {
        if self.edge_copy() {
            format_ident!("__edge_WS", span = Span::call_site())
        } else {
            format_ident!("WS", span = Span::call_site())
        }
    }

    /// Whether the tracked copy of a `dual` grammar is being generated.
    pub fn edge_copy(&self) -> bool {
        self.dual && self.streaming.get()
    }

    /// The name of a rule's inner parser: `parse_<rule>_inner`, or
    /// `__edge_parse_<rule>_inner` for the tracked copy.
    pub fn inner_name(&self, rule: &str, tracked: bool) -> syn::Ident {
        let span = Span::mixed_site();
        if tracked && self.dual {
            format_ident!("__edge_parse_{}_inner", rule, span = span)
        } else {
            format_ident!("parse_{}_inner", rule, span = span)
        }
    }

    /// Whether the grammar declares `input bytes;` - ADR 25.
    pub fn is_bytes(&self) -> bool {
        self.grammar.input == InputKind::Bytes
    }

    /// The input every rule of this grammar reads: `ParseInput`, or
    /// `ParseBytes` under `input bytes;`.
    pub fn input_ty(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::ParseBytes<'a, __WgS> }
        } else {
            quote! { ::winnow_grammar::ParseInput<'a, __WgS> }
        }
    }

    /// The bound on a rule's error type parameter `__WgEr`, for this grammar's
    /// input.
    pub fn rt_error(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::rt::RtError<'a, __WgS, &'a [u8]> }
        } else {
            quote! { ::winnow_grammar::rt::RtError<'a, __WgS> }
        }
    }

    /// Where the runtime helpers that yield what they matched live:
    /// `rt` for text, `rt::bytes` for a byte grammar. The helpers that only
    /// step and record are shared and stay `rt::…`.
    pub fn rts(&self) -> TokenStream {
        if self.is_bytes() {
            quote! { ::winnow_grammar::rt::bytes }
        } else {
            quote! { ::winnow_grammar::rt }
        }
    }
}

impl<'a> Codegen<'a> {
    pub fn new(grammar: &'a GrammarDefinition, frames: Frames, streams: Streams) -> Self {
        let user_rules = grammar.rules.iter().map(|r| r.name.to_string()).collect();
        let all_edges = std::env::var_os("WINNOW_GRAMMAR_EDGES").is_some();
        let dual = streams.any() && !all_edges;
        let tracked_rules = if dual {
            let roots = streams
                .paths
                .keys()
                .map(String::as_str)
                .chain(grammar.rules.iter().any(|r| r.name == "WS").then_some("WS"));
            winnow_grammar_model::frame::reached_rules(grammar, roots)
        } else {
            std::collections::HashSet::new()
        };
        let reach = edge::reaches(grammar, &streams);
        Self {
            streams,
            streaming: std::cell::Cell::new(all_edges),
            dual,
            tracked_rules,
            edge_k: edge::lookahead(grammar),
            reach,
            grammar,
            user_rules,
            input_ident: format_ident!("input", span = Span::call_site()),
            frames,
            current_boundary: RefCell::new(None),
            literal_rules: winnow_grammar_model::analysis::literal_rules(grammar),
            first_bytes: winnow_grammar_model::first_bytes::FirstBytes::new(grammar),
            cut_counter: std::cell::Cell::new(0),
        }
    }

    /// The plain copy of every rule - the only one, unless the grammar has a
    /// `#[stream]` rule - and then the tracked one of each rule a stream
    /// reaches (`dual`, `tracked_rules`, ADR 27 §3, *Per-mode copies*).
    fn generate_rules(&self) -> (Vec<TokenStream>, Vec<TokenStream>) {
        let rules: Vec<TokenStream> = self
            .grammar
            .rules
            .iter()
            .map(|r| self.generate_rule(r))
            .collect();
        let tracked: Vec<TokenStream> = if self.dual {
            self.streaming.set(true);
            let tracked = self
                .grammar
                .rules
                .iter()
                .filter(|r| self.tracked_rules.contains(&r.name.to_string()))
                .map(|r| self.generate_rule(r))
                .collect();
            self.streaming.set(false);
            tracked
        } else {
            Vec::new()
        };
        (rules, tracked)
    }

    /// The whitespace skip `WS` the generated parsers call between tokens:
    /// the user's `WS` rule, or the default scan. In a grammar generated
    /// twice (`dual`) also `__edge_WS`, the one the tracked copy calls.
    fn generate_ws(&self) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;
        let has_user_ws = self.user_rules.contains("WS");
        let input_ty = self.input_ty();
        let rt_error = self.rt_error();
        let rts = self.rts();

        let class = quote_spanned! {span=>
            #rts::class::<__WgS, __WgEr>(::winnow_grammar::ascii::AsciiClass::MULTISPACE, 0)
        };
        let default_ws = self.edge(class.clone(), None);

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
                fn WS<'a, __WgS: ::std::fmt::Debug + Clone, __WgEr: #rt_error>(
                    #input: &mut #input_ty,
                ) -> ::winnow::Result<(), ::winnow::error::ErrMode<__WgEr>> {
                    use ::winnow::Parser;
                    // The default whitespace skip runs between every two
                    // tokens, so it is the hottest scan in a generated parser
                    // - and a word at a time (`src/ascii.rs`), not a
                    // character at a time.
                    #default_ws
                    .parse_next(#input)
                    .map(|_| ())
                }
            }
        };
        if self.dual {
            let edge_ws = format_ident!("__edge_WS", span = Span::call_site());
            let edge_ws = if has_user_ws {
                let inner = self.inner_name("WS", true);
                quote_spanned! {span=>
                    #[allow(unused_imports)]
                    use #inner as #edge_ws;
                }
            } else {
                self.streaming.set(true);
                let tracked_ws = self.edge(class, None);
                self.streaming.set(false);
                quote_spanned! {span=>
                    #[allow(dead_code, non_snake_case)]
                    fn #edge_ws<'a, __WgS: ::std::fmt::Debug + Clone, __WgEr: #rt_error>(
                        #input: &mut #input_ty,
                    ) -> ::winnow::Result<(), ::winnow::error::ErrMode<__WgEr>> {
                        use ::winnow::Parser;
                        #tracked_ws
                        .parse_next(#input)
                        .map(|_| ())
                    }
                }
            };
            quote! { #ws_parser #edge_ws }
        } else {
            ws_parser
        }
    }

    pub fn generate(&mut self) -> syn::Result<TokenStream> {
        let grammar_name = &self.grammar.name;
        let span = Span::mixed_site();
        let use_statements = &self.grammar.uses;

        let (rules, tracked) = self.generate_rules();

        let use_super = quote_spanned! {Span::call_site()=> use super::*; };

        // `state T;`: a grammar-local extension trait, so that an action
        // writes `_state.user()` and gets a `&mut T` with no turbofish and no
        // second live borrow of the context - ADR 20.
        let interner_accessor = match &self.grammar.interner {
            Some(ty) => quote_spanned! {Span::call_site()=>
                trait __DeclaredInterner { fn declared_interner(&mut self) -> &mut #ty; }
                impl<__WgS: ::winnow_grammar::InternerOf<#ty>> __DeclaredInterner
                    for ::winnow_grammar::ParseContext<__WgS>
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
                impl<__WgS: ::winnow_grammar::StateOf<#ty>> __UserState
                    for ::winnow_grammar::ParseContext<__WgS>
                {
                    #[inline]
                    fn user(&mut self) -> &mut #ty {
                        ::winnow_grammar::StateOf::state(&mut self.user_state)
                    }
                }
            },
            None => quote! {},
        };

        let ws_parser = self.generate_ws();

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
                #(#tracked)*
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Codegen;
    use quote::quote;
    use winnow_grammar_model::parse_grammar;

    /// The rules of `rules` that get a tracked copy (ADR 27 §3, *Per-mode
    /// copies*), sorted; `None` where the grammar is not generated twice.
    fn tracked(rules: proc_macro2::TokenStream) -> Option<Vec<String>> {
        let parsed = parse_grammar::<crate::WinnowBackend>(quote! {
            grammar G { #rules }
        })
        .unwrap();
        let cg = Codegen::new(&parsed.grammar, parsed.frames, parsed.streams);
        if !cg.dual {
            return None;
        }
        let mut names: Vec<String> = cg.tracked_rules.iter().cloned().collect();
        names.sort();
        Some(names)
    }

    #[test]
    fn only_what_a_stream_reaches_gets_a_tracked_copy() {
        // `WINNOW_GRAMMAR_EDGES` asks for one tracked copy of everything.
        if std::env::var_os("WINNOW_GRAMMAR_EDGES").is_some() {
            return;
        }
        let rules = quote! {
            #[stream]
            pub list -> i64 = "[" s:fold(num, || 0i64, |a: i64, n: i64| a + n) "]" -> { s }
            num -> i64 = n:i64 ","? peek(digit)? -> { n }
            pub other -> i64 = "(" n:alone ")" -> { n }
            alone -> i64 = n:i64 -> { n }
        };
        assert_eq!(tracked(rules), Some(vec!["list".into(), "num".into()]));
    }

    #[test]
    fn the_grammar_s_own_ws_always_gets_one() {
        if std::env::var_os("WINNOW_GRAMMAR_EDGES").is_some() {
            return;
        }
        let rules = quote! {
            WS -> () = " "* -> { }
            #[stream]
            pub list -> i64 = s:fold(num, || 0i64, |a: i64, n: i64| a + n) -> { s }
            num -> i64 = n:i64 ";" -> { n }
        };
        assert_eq!(
            tracked(rules),
            Some(vec!["WS".into(), "list".into(), "num".into()])
        );
    }

    #[test]
    fn a_grammar_without_a_stream_rule_is_generated_once() {
        assert_eq!(tracked(quote! { pub n -> i64 = n:i64 -> { n } }), None);
    }
}
