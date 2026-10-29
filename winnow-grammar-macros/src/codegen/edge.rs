//! Edge tracking for a grammar with a `#[stream]` rule (ADR 27 §3): every
//! parser the generator writes is wrapped in `rt::edge`, which notes in the
//! context when the parser may have depended on where its input ends.
//!
//! The note is sound when `k` is at least how far any parser of the grammar
//! looks past the position it stops at, and past the position it began at
//! when it fails. Every position a parse reaches it reaches through a parser
//! that stopped there, and that parser checked `k` bytes; so whatever looks
//! at most `k` bytes on from there was checked already. Only a scan with no
//! bound - `string`'s search for its closing quote - fails further away, and
//! it has a check of its own (`rt::edge_quoted`).

use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use winnow_grammar_model::model::{Argument, GrammarDefinition, ModelPattern};

/// The `k` of a grammar: the longest literal, and the furthest a built-in it
/// uses looks.
pub fn lookahead(grammar: &GrammarDefinition) -> usize {
    let mut k = 1;
    for r in &grammar.rules {
        for v in &r.variants {
            for p in &v.pattern {
                walk(p, &mut k);
            }
        }
    }
    k
}

fn walk(p: &ModelPattern, k: &mut usize) {
    match p {
        ModelPattern::Lit { lit, .. } => {
            let n = match lit {
                syn::Lit::Str(s) => s.value().len(),
                syn::Lit::Char(c) => c.value().len_utf8(),
                _ => 1,
            };
            *k = (*k).max(n);
        }
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            let n = match rule_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .as_deref()
            {
                // A character decoded from UTF-8, or a run that stops at
                // one: up to four bytes.
                Some("any" | "alpha1" | "ident" | "raw_ident" | "string") => 4,
                // `'\n'`, `'𝄞'`: the quotes and four bytes.
                Some("char") => 6,
                // `infinity`.
                Some("f32" | "f64") => 8,
                Some("bool") => 5,
                Some("line_ending") => 2,
                Some(
                    "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64"
                    | "i128" | "isize",
                ) => 2,
                _ => 1,
            };
            *k = (*k).max(n);
            for a in args {
                if let Argument::Positional(q) | Argument::Named(_, q) = a {
                    walk(q, k);
                }
            }
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for q in seq {
                    walk(q, k);
                }
            }
        }
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => {
            for q in seq {
                walk(q, k);
            }
        }
        ModelPattern::Optional(q, _)
        | ModelPattern::Repeat(q, _)
        | ModelPattern::Plus(q, _)
        | ModelPattern::SpanBinding(q, _, _)
        | ModelPattern::Peek(q, _)
        | ModelPattern::Not(q, _)
        | ModelPattern::LexicalScope(q, _)
        | ModelPattern::SpacedScope(q, _) => walk(q, k),
        ModelPattern::Bounded { pattern, .. }
        | ModelPattern::Until { pattern, .. }
        | ModelPattern::Count { pattern, .. }
        | ModelPattern::Fold { pattern, .. } => walk(pattern, k),
        ModelPattern::Recover { body, sync, .. } => {
            walk(body, k);
            walk(sync, k);
        }
        ModelPattern::Cut(_) | ModelPattern::Fail { .. } | ModelPattern::Guard { .. } => {}
    }
}

impl Codegen<'_> {
    /// `p` wrapped in `rt::edge`, in a grammar that tracks edges; `p` as it
    /// is otherwise. `pattern` is what it parses, where that is known.
    pub fn edge(&self, p: TokenStream, pattern: Option<&ModelPattern>) -> TokenStream {
        if !self.streaming {
            return p;
        }
        let span = Span::mixed_site();
        let k = self.edge_k;
        let quoted = matches!(pattern, Some(ModelPattern::RuleCall { rule_path, .. })
            if rule_path.is_ident("string") && !self.user_rules.contains("string"));
        if quoted {
            quote_spanned! {span=> ::winnow_grammar::rt::edge_quoted(#k, b'"', #p) }
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::edge(#k, #p) }
        }
    }

    /// `+ input.state.origin` where spans are moved to the whole input's
    /// positions - in a stream's steps.
    pub fn origin(&self) -> TokenStream {
        if !self.streaming {
            return TokenStream::new();
        }
        let input = &self.input_ident;
        quote_spanned! {Span::mixed_site()=> + #input.state.origin }
    }
}
