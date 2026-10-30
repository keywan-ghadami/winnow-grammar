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

/// Whether an item of a stream may record a failure past its own end, so
/// that an error further on has to be diagnosed with it (ADR 27 §3,
/// *Errors*). The parse in memory records a failure where a repetition, an
/// option, a `fold`, a `recover` or a rule's alternatives swallow it; one of
/// them reaches past where the item ends only when the attempt it gave up
/// could have read more than one token first. A grammar without such an
/// attempt in what its stream rules reach - rows, `n:i64 ","?` - records
/// nothing an item further on has to beat, and is generated without the
/// tracking (`rt::edge_reach`) that tells which items do.
///
/// Conservative: a call of a rule of the grammar, a hand-written parser and
/// `string` (whose errors may lie inside it) count as reading more than a
/// token, and lookahead counts like anything else.
pub fn reaches(
    grammar: &GrammarDefinition,
    streams: &winnow_grammar_model::stream::Streams,
) -> bool {
    if !streams.any() {
        return false;
    }
    use std::collections::HashMap;
    let rules: HashMap<String, &winnow_grammar_model::model::Rule> = grammar
        .rules
        .iter()
        .map(|r| (r.name.to_string(), r))
        .collect();
    let mut todo: Vec<String> = streams.paths.keys().cloned().collect();
    // The implicit whitespace skip is reached by every syntactic rule.
    if rules.contains_key("WS") {
        todo.push("WS".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(rule) = rules.get(&name) else {
            continue;
        };
        if rule.variants.len() > 1
            && rule
                .variants
                .iter()
                .any(|v| !begins_only(&v.pattern, &rules))
        {
            return true;
        }
        // The fold a stream hands out: its last attempt ends it, and the
        // driver diagnoses that one itself. Only its item is looked into.
        let fold_at = streams
            .paths
            .values()
            .filter_map(|p| p.levels.last())
            .find(|l| l.rule == name)
            .map(|l| l.at);
        for v in &rule.variants {
            for (i, p) in v.pattern.iter().enumerate() {
                let p = match p {
                    ModelPattern::Fold { pattern, .. } if Some(i) == fold_at => &**pattern,
                    p => p,
                };
                if swallows_far(p, &rules, &mut todo) {
                    return true;
                }
            }
        }
    }
    false
}

/// A sequence that fails only where it begins: its first element does, and
/// what follows cannot fail.
fn begins_only(
    seq: &[ModelPattern],
    rules: &std::collections::HashMap<String, &winnow_grammar_model::model::Rule>,
) -> bool {
    let mut rest = seq.iter().skip_while(|p| matches!(p, ModelPattern::Cut(_)));
    match rest.next() {
        None => true,
        Some(first) => atomic(first, rules) && rest.all(infallible),
    }
}

/// A pattern that matches, if only nothing.
fn infallible(p: &ModelPattern) -> bool {
    match p {
        ModelPattern::Optional(..)
        | ModelPattern::Repeat(..)
        | ModelPattern::Count { .. }
        | ModelPattern::Fold { .. }
        | ModelPattern::Cut(_) => true,
        ModelPattern::Bounded { min, dynamic, .. } => *min == 0 && dynamic.is_none(),
        ModelPattern::SpanBinding(q, _, _)
        | ModelPattern::LexicalScope(q, _)
        | ModelPattern::SpacedScope(q, _) => infallible(q),
        _ => false,
    }
}

/// A pattern that fails only where it begins: a literal, a built-in token,
/// or a group of sequences that do.
fn atomic(
    p: &ModelPattern,
    rules: &std::collections::HashMap<String, &winnow_grammar_model::model::Rule>,
) -> bool {
    match p {
        ModelPattern::Lit { .. } => true,
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            let name = rule_path.segments.last().map(|s| s.ident.to_string());
            rule_path.segments.len() == 1
                && args.is_empty()
                && !name.as_ref().is_some_and(|n| rules.contains_key(n))
                && !matches!(name.as_deref(), Some("string") | None)
        }
        ModelPattern::SpanBinding(q, _, _)
        | ModelPattern::LexicalScope(q, _)
        | ModelPattern::SpacedScope(q, _) => atomic(q, rules),
        ModelPattern::Group { alts, .. } => alts.iter().all(|(seq, _, _)| begins_only(seq, rules)),
        _ => false,
    }
}

/// Does `p` hold a swallowing point whose attempt is not atomic? Collects the
/// rules it calls.
fn swallows_far(
    p: &ModelPattern,
    rules: &std::collections::HashMap<String, &winnow_grammar_model::model::Rule>,
    todo: &mut Vec<String>,
) -> bool {
    let attempt = |q: &ModelPattern| !atomic(q, rules);
    match p {
        ModelPattern::Optional(q, _) | ModelPattern::Repeat(q, _) | ModelPattern::Plus(q, _) => {
            attempt(q) || swallows_far(q, rules, todo)
        }
        ModelPattern::Bounded { pattern, .. }
        | ModelPattern::Count { pattern, .. }
        | ModelPattern::Fold { pattern, .. } => {
            attempt(pattern) || swallows_far(pattern, rules, todo)
        }
        ModelPattern::Recover { .. } => true,
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            if let Some(s) = rule_path.segments.last() {
                if rules.contains_key(&s.ident.to_string()) {
                    todo.push(s.ident.to_string());
                }
            }
            args.iter().any(|a| match a {
                Argument::Positional(q) | Argument::Named(_, q) => swallows_far(q, rules, todo),
                _ => false,
            })
        }
        ModelPattern::Group { alts, .. } => alts
            .iter()
            .any(|(seq, _, _)| seq.iter().any(|q| swallows_far(q, rules, todo))),
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => seq.iter().any(|q| swallows_far(q, rules, todo)),
        ModelPattern::SpanBinding(q, _, _)
        | ModelPattern::Peek(q, _)
        | ModelPattern::Not(q, _)
        | ModelPattern::LexicalScope(q, _)
        | ModelPattern::SpacedScope(q, _) => swallows_far(q, rules, todo),
        ModelPattern::Until { pattern, .. } => swallows_far(pattern, rules, todo),
        ModelPattern::Lit { .. }
        | ModelPattern::Cut(_)
        | ModelPattern::Fail { .. }
        | ModelPattern::Guard { .. } => false,
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
        let edge = if self.reach {
            quote_spanned! {span=> ::winnow_grammar::rt::edge_reach }
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::edge }
        };
        if quoted {
            quote_spanned! {span=> ::winnow_grammar::rt::edge_quoted(#k, b'"', #edge(#k, #p)) }
        } else {
            quote_spanned! {span=> #edge(#k, #p) }
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
