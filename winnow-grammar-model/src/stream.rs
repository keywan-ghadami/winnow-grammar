//! `#[stream]`: a rule whose input may be read from a `Read` in bounded
//! memory, one item of a `fold` at a time. The decision record is
//! `docs/adr/adr27-streaming-input.md` §3.
//!
//! A stream rule reaches one `fold(ITEM, init, step)` through a **path**: the
//! rule itself holds the fold, or holds one call of a rule that does, and so
//! on. Every rule on the path has one alternative, so nothing is open around
//! the fold when its items are handed out, and the parser can stop between
//! two items, drop the input it has read, and go on with the next block.
//!
//! What crosses such a stop must not borrow the input: the values a rule on
//! the path bound before the call it continues in, and wants after it. The
//! code generator keeps them between blocks, so their types must be known -
//! a rule's declared return type, or a built-in whose type is fixed. This
//! module finds the path, checks it, and says what crosses where.

use crate::model::*;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};

/// The attribute that marks a stream rule.
pub const ATTR: &str = "stream";

/// What the stream check established, for the code generator.
#[derive(Debug, Clone, Default)]
pub struct Streams {
    /// Stream rules by name.
    pub paths: HashMap<String, StreamPath>,
}

/// The rules from a stream rule down to the one that holds the fold.
#[derive(Debug, Clone)]
pub struct StreamPath {
    pub levels: Vec<Level>,
    /// `#[stream(each)]`: the rule also gets `parse_<rule>_reader_each`,
    /// which shows the caller every item as it is taken.
    pub each: bool,
}

/// One rule of a path.
#[derive(Debug, Clone)]
pub struct Level {
    pub rule: String,
    /// The index, in the rule's one alternative, of the element the path
    /// continues in: the call of the next rule, or, in the last rule, the
    /// fold.
    pub at: usize,
    /// Values bound before `at` and used after it, with their types.
    pub carried: Vec<(syn::Ident, syn::Type)>,
    /// Values bound after `at` and used by the action, with their types.
    pub returned: Vec<(syn::Ident, syn::Type)>,
}

impl Streams {
    pub fn any(&self) -> bool {
        !self.paths.is_empty()
    }
}

/// `None` for a rule that is not a stream rule; `Some(each)` for one, `each`
/// being whether `#[stream(each)]` asks for the reader that shows every item.
fn is_stream(rule: &Rule) -> syn::Result<Option<bool>> {
    for a in &rule.attrs {
        if a.path().is_ident(ATTR) {
            return match &a.meta {
                syn::Meta::Path(_) => Ok(Some(false)),
                syn::Meta::List(l) if l.parse_args::<syn::Ident>().is_ok_and(|i| i == "each") => {
                    Ok(Some(true))
                }
                _ => Err(syn::Error::new_spanned(
                    a,
                    "`#[stream]` takes no arguments but `each`: the path to the fold is found, \
                     not named; `#[stream(each)]` adds `parse_<rule>_reader_each`",
                )),
            };
        }
    }
    Ok(None)
}

/// Finds and checks the path of every `#[stream]` rule.
pub fn check(grammar: &GrammarDefinition, frames: &crate::frame::Frames) -> syn::Result<Streams> {
    let rules: HashMap<String, &Rule> = grammar
        .rules
        .iter()
        .map(|r| (r.name.to_string(), r))
        .collect();
    let mut streams = Streams::default();
    for rule in &grammar.rules {
        let Some(each) = is_stream(rule)? else {
            continue;
        };
        let name = rule.name.to_string();
        if frames.par_folds.contains_key(&name) || frames.frames.contains_key(&name) {
            return Err(syn::Error::new(
                rule.name.span(),
                format!(
                    "rule `{name}` is a frame or a `par_fold`, which `parse_{name}_reader` already \
                     reads from a stream by its frames (ADR 27 §1); `#[stream]` is for a grammar \
                     whose items are not cut at a boundary"
                ),
            ));
        }
        let mut levels = Vec::new();
        let mut seen = HashSet::new();
        let mut current: &Rule = rule;
        loop {
            if !seen.insert(current.name.to_string()) {
                return Err(err(
                    current.name.span(),
                    &name,
                    format!("the path returns to rule `{}`", current.name),
                ));
            }
            let (at, last) = step(current, &rules, &name)?;
            levels.push(Level {
                rule: current.name.to_string(),
                at,
                carried: Vec::new(),
                returned: Vec::new(),
            });
            match last {
                Some(next) => current = next,
                None => break,
            }
        }
        for level in &mut levels {
            let r = rules[&level.rule];
            let (carried, returned) = crossings(r, level.at, &rules, grammar, &name)?;
            level.carried = carried;
            level.returned = returned;
        }
        streams.paths.insert(name, StreamPath { levels, each });
    }
    Ok(streams)
}

fn err(span: Span, stream: &str, what: String) -> syn::Error {
    syn::Error::new(
        span,
        format!("{what}; the stream rule `{stream}` passes through it (ADR 27 §3)"),
    )
}

/// Checks one rule of the path and finds where it continues: the index of
/// the element, and the next rule, `None` when the element is the fold.
fn step<'g>(
    rule: &'g Rule,
    rules: &HashMap<String, &'g Rule>,
    stream: &str,
) -> syn::Result<(usize, Option<&'g Rule>)> {
    let name = &rule.name;
    let fail = |what: String| Err(err(name.span(), stream, what));
    if rule.variants.len() != 1 {
        return fail(format!(
            "rule `{name}` has {} alternatives, and a rule on a stream's path has one: an item \
             handed out cannot be taken back by an alternative that loses",
            rule.variants.len()
        ));
    }
    if !rule.generics.params.is_empty() {
        return fail(format!("rule `{name}` is generic"));
    }
    if rule.params.iter().any(|p| !is_value(p)) {
        return fail(format!(
            "rule `{name}` takes a parser and is inlined where it is called"
        ));
    }
    let v = &rule.variants[0];
    if v.with_span || rule.label.is_some() || v.label.is_some() {
        return fail(format!(
            "rule `{name}` takes a span or a label over its whole body, which a stream does not \
             keep"
        ));
    }
    let mut found: Vec<(usize, Option<&'g Rule>)> = Vec::new();
    for (i, p) in v.pattern.iter().enumerate() {
        match p {
            ModelPattern::Fold { merge: None, .. } => found.push((i, None)),
            ModelPattern::RuleCall { rule_path, .. } => {
                if let Some(next) = rule_path
                    .get_ident()
                    .and_then(|id| rules.get(&id.to_string()))
                {
                    if reaches_fold(next, rules, &mut HashSet::new()) {
                        found.push((i, Some(next)));
                    }
                }
            }
            _ => {}
        }
    }
    match found.as_slice() {
        [] => fail(format!(
            "rule `{name}` neither holds a `fold(…)` as an element of its own nor calls a rule \
             that leads to one"
        )),
        [(i, None)] => {
            let ModelPattern::Fold {
                pattern, binding, ..
            } = &v.pattern[*i]
            else {
                unreachable!()
            };
            match &**pattern {
                ModelPattern::RuleCall { rule_path, .. }
                    if rule_path
                        .get_ident()
                        .and_then(|id| rules.get(&id.to_string()))
                        .is_some_and(|r| r.params.iter().all(is_value)) => {}
                other => {
                    return Err(err(
                        other.span(),
                        stream,
                        "the item of a streamed `fold` is a call of a rule of the grammar, whose \
                         declared type is what the step receives"
                            .to_string(),
                    ))
                }
            }
            if let Some(b) = binding {
                let mut used = HashSet::new();
                for p in &v.pattern[i + 1..] {
                    idents_of_pattern(p, &mut used);
                }
                if used.contains(&b.to_string()) {
                    return Err(err(
                        b.span(),
                        stream,
                        format!(
                            "`{b}`, the fold's value, is used by an element after the fold; it \
                             is known only to the action"
                        ),
                    ));
                }
            }
            Ok((*i, None))
        }
        [one] => Ok(*one),
        _ => fail(format!(
            "rule `{name}` has more than one element that leads to a `fold(…)`; a stream goes \
             through exactly one"
        )),
    }
}

fn is_value(p: &RuleParameter) -> bool {
    match &p.ty {
        None => false,
        Some(syn::Type::Path(t)) => t.path.segments.last().is_none_or(|s| s.ident != "Rule"),
        Some(_) => true,
    }
}

fn reaches_fold(rule: &Rule, rules: &HashMap<String, &Rule>, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(rule.name.to_string()) || rule.variants.len() != 1 {
        return false;
    }
    rule.variants[0].pattern.iter().any(|p| match p {
        ModelPattern::Fold { merge: None, .. } => true,
        ModelPattern::RuleCall { rule_path, .. } => rule_path
            .get_ident()
            .and_then(|id| rules.get(&id.to_string()))
            .is_some_and(|r| reaches_fold(r, rules, seen)),
        _ => false,
    })
}

/// What crosses a stop in `rule`: the values bound before `at` that are used
/// after it, and those bound after it that the action uses - with types.
#[allow(clippy::type_complexity)]
fn crossings(
    rule: &Rule,
    at: usize,
    rules: &HashMap<String, &Rule>,
    grammar: &GrammarDefinition,
    stream: &str,
) -> syn::Result<(Vec<(syn::Ident, syn::Type)>, Vec<(syn::Ident, syn::Type)>)> {
    let v = &rule.variants[0];
    let mut action = HashSet::new();
    idents_of_tokens(&v.action, &mut action);
    // What is used after the prefix: by the path element (a fold's item
    // arguments, init and step are evaluated between blocks), the suffix
    // and the action. The arguments of a call that continues the path are
    // evaluated in the prefix, so they do not count.
    let mut after = action.clone();
    match &v.pattern[at] {
        p @ ModelPattern::Fold { .. } => idents_of_pattern(p, &mut after),
        ModelPattern::RuleCall { .. } => {}
        _ => unreachable!(),
    }
    for p in &v.pattern[at + 1..] {
        idents_of_pattern(p, &mut after);
    }
    let typed = |bound: Vec<(syn::Ident, &ModelPattern)>, used: &HashSet<String>| {
        let mut out = Vec::new();
        for (id, p) in bound {
            if !used.contains(&id.to_string()) {
                continue;
            }
            match binding_type(p, rules, grammar) {
                Some(ty) if !borrows(&ty.to_token_stream()) => out.push((id, ty)),
                Some(ty) => {
                    return Err(err(
                        id.span(),
                        stream,
                        format!(
                            "`{id}` is kept from one block of input to the next, and its type \
                             `{}` borrows the input",
                            ty.to_token_stream().to_string().replace("& '", "&'")
                        ),
                    ))
                }
                None => {
                    return Err(err(
                        id.span(),
                        stream,
                        format!(
                            "`{id}` is kept from one block of input to the next, so its type \
                             must be known: bind a call of a rule (its declared type) or a \
                             built-in of fixed type (`ident`, `u32`, `dec<T>(…)`, `count(…)`, …)"
                        ),
                    ))
                }
            }
        }
        Ok(out)
    };
    let mut before = Vec::new();
    for p in &v.pattern[..at] {
        bindings_of(p, &mut before);
    }
    let mut later = Vec::new();
    for p in &v.pattern[at + 1..] {
        bindings_of(p, &mut later);
    }
    let carried = typed(before, &after)?;
    let returned = typed(later, &action)?;
    for p in &rule.params {
        if let Some(ty) = &p.ty {
            if borrows(&ty.to_token_stream()) {
                return Err(err(
                    p.name.span(),
                    stream,
                    format!(
                        "parameter `{}` of rule `{}` borrows the input",
                        p.name, rule.name
                    ),
                ));
            }
        }
    }
    if borrows(&rule.return_type.to_token_stream()) {
        return Err(err(
            rule.name.span(),
            stream,
            format!(
                "rule `{}` returns a type that borrows the input, and its value is made after \
                 the input it was read from is gone",
                rule.name
            ),
        ));
    }
    Ok((carried, returned))
}

/// The bindings an element of a sequence makes visible to what follows it.
fn bindings_of<'p>(p: &'p ModelPattern, out: &mut Vec<(syn::Ident, &'p ModelPattern)>) {
    match p {
        ModelPattern::Lit {
            binding: Some(b), ..
        }
        | ModelPattern::RuleCall {
            binding: Some(b), ..
        }
        | ModelPattern::Group {
            binding: Some(b), ..
        }
        | ModelPattern::Recover {
            binding: Some(b), ..
        }
        | ModelPattern::Until {
            binding: Some(b), ..
        }
        | ModelPattern::Count {
            binding: Some(b), ..
        }
        | ModelPattern::Fold {
            binding: Some(b), ..
        } => out.push((b.clone(), p)),
        ModelPattern::SpanBinding(inner, span, _) => {
            bindings_of(inner, out);
            out.push((span.clone(), p));
        }
        ModelPattern::Group {
            binding: None,
            alts,
            ..
        } if alts.len() == 1 => {
            for q in &alts[0].0 {
                bindings_of(q, out);
            }
        }
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => {
            for q in seq {
                bindings_of(q, out);
            }
        }
        _ => {}
    }
}

/// The type of what `p` binds, where it is known without inference.
fn binding_type(
    p: &ModelPattern,
    rules: &HashMap<String, &Rule>,
    grammar: &GrammarDefinition,
) -> Option<syn::Type> {
    match p {
        ModelPattern::Count { .. } => Some(syn::parse_quote!(usize)),
        ModelPattern::SpanBinding(..) => Some(syn::parse_quote!(::core::ops::Range<usize>)),
        ModelPattern::RuleCall {
            rule_path,
            generics,
            ..
        } => {
            let name = rule_path.get_ident()?.to_string();
            if let Some(r) = rules.get(&name) {
                return r
                    .params
                    .iter()
                    .all(is_value)
                    .then(|| r.return_type.clone())
                    .filter(|_| r.generics.params.is_empty());
            }
            if let Some(e) = grammar.extern_rules.iter().find(|e| e.name == name) {
                return e.generics.params.is_empty().then(|| e.return_type.clone());
            }
            match name.as_str() {
                "ident" | "intern" => Some(syn::parse_quote!(::winnow_grammar::Symbol)),
                "char" | "any" => Some(syn::parse_quote!(char)),
                "byte" => Some(syn::parse_quote!(u8)),
                "dec" => generics.first().cloned(),
                n @ ("u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32"
                | "i64" | "i128" | "isize" | "f32" | "f64" | "bool") => syn::parse_str(n).ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Does a type name a lifetime other than `'static`?
pub fn borrows(ty: &TokenStream) -> bool {
    let mut tokens = ty.clone().into_iter().peekable();
    while let Some(t) = tokens.next() {
        match t {
            TokenTree::Punct(p) if p.as_char() == '\'' => {
                if let Some(TokenTree::Ident(id)) = tokens.peek() {
                    if id != "static" {
                        return true;
                    }
                }
            }
            TokenTree::Group(g) if borrows(&g.stream()) => return true,
            _ => {}
        }
    }
    false
}

/// Every identifier in a token stream - a superset of the values it uses.
pub fn idents_of_tokens(ts: &TokenStream, out: &mut HashSet<String>) {
    for t in ts.clone() {
        match t {
            TokenTree::Ident(id) => {
                out.insert(id.to_string());
            }
            TokenTree::Group(g) => idents_of_tokens(&g.stream(), out),
            _ => {}
        }
    }
}

/// Every identifier a pattern's expressions mention: arguments, guards,
/// computed bounds, a fold's init and step.
pub fn idents_of_pattern(p: &ModelPattern, out: &mut HashSet<String>) {
    match p {
        ModelPattern::RuleCall { args, .. } => {
            for a in args {
                match a {
                    Argument::Value(_, e) => idents_of_tokens(&e.to_token_stream(), out),
                    Argument::Positional(q) | Argument::Named(_, q) => idents_of_pattern(q, out),
                }
            }
        }
        ModelPattern::Guard { cond, .. } => idents_of_tokens(&cond.to_token_stream(), out),
        ModelPattern::Bounded {
            pattern, dynamic, ..
        } => {
            if let Some(d) = dynamic {
                idents_of_tokens(&d.min.to_token_stream(), out);
                if let crate::parser::DynMax::To(e) = &d.max {
                    idents_of_tokens(&e.to_token_stream(), out);
                }
            }
            idents_of_pattern(pattern, out);
        }
        ModelPattern::Fold {
            pattern,
            init,
            step,
            ..
        } => {
            idents_of_pattern(pattern, out);
            idents_of_tokens(&init.to_token_stream(), out);
            idents_of_tokens(&step.to_token_stream(), out);
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for q in seq {
                    idents_of_pattern(q, out);
                }
            }
        }
        ModelPattern::Bracketed(seq, _)
        | ModelPattern::Braced(seq, _)
        | ModelPattern::Parenthesized(seq, _) => {
            for q in seq {
                idents_of_pattern(q, out);
            }
        }
        ModelPattern::Optional(q, _)
        | ModelPattern::Repeat(q, _)
        | ModelPattern::Plus(q, _)
        | ModelPattern::SpanBinding(q, _, _)
        | ModelPattern::Peek(q, _)
        | ModelPattern::Not(q, _)
        | ModelPattern::LexicalScope(q, _)
        | ModelPattern::SpacedScope(q, _) => idents_of_pattern(q, out),
        ModelPattern::Until { pattern, .. } | ModelPattern::Count { pattern, .. } => {
            idents_of_pattern(pattern, out)
        }
        ModelPattern::Recover { body, sync, .. } => {
            idents_of_pattern(body, out);
            idents_of_pattern(sync, out);
        }
        ModelPattern::Cut(_) | ModelPattern::Lit { .. } | ModelPattern::Fail { .. } => {}
    }
}
