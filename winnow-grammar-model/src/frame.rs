//! Frames: rules a parser can resynchronize on, and what that costs the rest
//! of the grammar.
//!
//! A rule marked `#[frame]` claims that an occurrence of it can be found from
//! an arbitrary offset in the input by scanning to the next **boundary** — the
//! literal it ends in, or the one given as `#[frame = "\n"]` — with no parse
//! state from earlier in the input. That is what lets a large input be cut
//! blindly into pieces, each piece repair its own start to the next boundary,
//! and every frame end up in exactly one piece.
//!
//! The claim is only true if the boundary cannot occur *inside* a frame. That
//! is not taken on trust. Every rule reachable from the frame rule is walked,
//! and each thing that consumes input is one of three cases:
//!
//! * **Safe** — a literal that does not contain the boundary, a built-in whose
//!   alphabet cannot include it (`digit1`, `ident`, …), something that does not
//!   consume at all (`peek`, `not`, `eof`).
//! * **Bounded** — `until(t)` and the skip inside `recover(…)`. These consume
//!   *anything* up to their terminator, so on their own they would run through
//!   a boundary. Rather than reject them, the generated scan is bounded: it
//!   stops at the terminator *or the boundary*, whichever comes first. A name
//!   with a stray newline in it then fails to parse where the newline is,
//!   instead of silently joining two frames. This is what [`Frames::bounded`]
//!   records for the code generator.
//! * **Rejected** — a literal that contains the boundary (`"\n"` inside a
//!   quoted-string rule: the CSV-with-quoted-newlines case), a built-in whose
//!   alphabet includes it (`any`, `multispace0`), or the implicit whitespace of
//!   a syntactic (lowercase) rule when the grammar's whitespace can consume it.
//!   Each is a compile error naming the rule and the pattern, because cutting
//!   such an input at the next boundary would land inside a record on some
//!   inputs and not others.
//!
//! `par_fold(rule, init, step, merge)` is the second half: it requires `rule`
//! to be a frame and supplies the merge, so the pieces can be folded
//! independently and combined. This module also checks that use.

use crate::model::{Argument, GrammarDefinition, ModelPattern, Rule};
use std::collections::{BTreeMap, HashSet};
use syn::spanned::Spanned;

/// The `#[frame]` attribute as written on a rule.
#[derive(Debug, Clone)]
pub struct FrameAttr {
    /// `#[frame = "\n"]` / `#[frame("\n")]`; `None` for a bare `#[frame]`.
    pub boundary: Option<String>,
    pub span: proc_macro2::Span,
}

/// Reads `#[frame]`, `#[frame = "…"]` and `#[frame("…")]` off a rule.
pub fn frame_attr(rule: &Rule) -> syn::Result<Option<FrameAttr>> {
    let mut found: Option<FrameAttr> = None;
    for attr in &rule.attrs {
        if !attr.path().is_ident("frame") {
            continue;
        }
        let span = attr.span();
        let boundary = match &attr.meta {
            syn::Meta::Path(_) => None,
            syn::Meta::NameValue(nv) => Some(boundary_literal(&nv.value)?),
            syn::Meta::List(list) => Some(boundary_literal(&list.parse_args::<syn::Expr>()?)?),
        };
        if found.is_some() {
            return Err(syn::Error::new(span, "`#[frame]` given twice"));
        }
        found = Some(FrameAttr { boundary, span });
    }
    Ok(found)
}

fn boundary_literal(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) if !s.value().is_empty() => Ok(s.value()),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Char(c),
            ..
        }) => Ok(c.value().to_string()),
        _ => Err(syn::Error::new(
            expr.span(),
            "the frame boundary is a non-empty string or char literal: `#[frame = \"\\n\"]`",
        )),
    }
}

/// What the frame check establishes, for the code generator.
#[derive(Debug, Default, Clone)]
pub struct Frames {
    /// Frame rule → its boundary.
    pub frames: BTreeMap<String, String>,
    /// Every rule reachable from a frame rule, the frame rule included, → the
    /// boundary its `until`/`recover` skips must stop at. A rule reached from
    /// a frame is part of that frame's format.
    pub bounded: BTreeMap<String, String>,
    /// Rules whose body is a `par_fold` → the frame rule it folds over.
    pub par_folds: BTreeMap<String, String>,
}

impl Frames {
    /// The boundary that bounds skips inside `rule`, if any.
    pub fn boundary_for(&self, rule: &str) -> Option<&str> {
        self.bounded.get(rule).map(String::as_str)
    }
}

/// Runs the frame check over the whole grammar. `builtins` are the names the
/// backend provides; anything not a user rule and not a built-in is left to
/// the ordinary "undefined rule" validation.
pub fn check(grammar: &GrammarDefinition, builtins: &HashSet<String>) -> syn::Result<Frames> {
    let mut out = Frames::default();
    let user_rules: HashSet<String> = grammar.rules.iter().map(|r| r.name.to_string()).collect();
    let has_user_ws = user_rules.contains("WS");

    // 1. Frame rules and their boundaries.
    for rule in &grammar.rules {
        let Some(attr) = frame_attr(rule)? else {
            continue;
        };
        let boundary = match attr.boundary {
            Some(b) => b,
            None => infer_boundary(rule, attr.span)?,
        };
        check_terminators(rule, &boundary, &user_rules)?;
        out.frames.insert(rule.name.to_string(), boundary);
    }

    // 2. Reachability and the per-rule check.
    for (frame_name, boundary) in &out.frames {
        let frame_rule = grammar
            .rules
            .iter()
            .find(|r| r.name == frame_name)
            .expect("frame rule exists");

        let mut reachable: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut stack = vec![frame_name.clone()];
        while let Some(name) = stack.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            reachable.push(name.clone());
            let Some(rule) = grammar.rules.iter().find(|r| r.name == name) else {
                continue;
            };
            let mut calls = Vec::new();
            for v in &rule.variants {
                collect_calls(&v.pattern, &user_rules, &mut calls);
            }
            // A syntactic rule calls the grammar's own `WS` between its
            // elements without naming it.
            if has_user_ws && !rule.is_lexical {
                calls.push("WS".to_string());
            }
            stack.extend(calls);
        }

        for name in &reachable {
            if let Some(other) = out.bounded.get(name) {
                if other != boundary {
                    return Err(syn::Error::new(
                        frame_rule.name.span(),
                        format!(
                            "rule `{name}` is reached from two frames with different boundaries \
                             ({:?} and {:?}); a rule is part of one frame's format",
                            other, boundary
                        ),
                    ));
                }
            }
            out.bounded.insert(name.clone(), boundary.clone());

            let rule = grammar
                .rules
                .iter()
                .find(|r| r.name == name)
                .expect("reachable rules are user rules");
            let cx = Cx {
                frame: frame_name,
                boundary,
                user_rules: &user_rules,
                builtins,
                has_user_ws,
            };
            check_rule(rule, name == frame_name, &cx)?;
        }
    }

    // 3. `par_fold` uses.
    for rule in &grammar.rules {
        check_par_fold(rule, &out.frames, &mut out.par_folds)?;
    }

    Ok(out)
}

// -----------------------------------------------------------------------------
// Boundary inference and the terminator position
// -----------------------------------------------------------------------------

/// A bare `#[frame]`: the boundary is the literal every variant ends in.
fn infer_boundary(rule: &Rule, attr_span: proc_macro2::Span) -> syn::Result<String> {
    let mut found: Option<String> = None;
    for v in &rule.variants {
        let last = v.pattern.last().and_then(literal_text);
        match (last, &found) {
            (Some(b), None) => found = Some(b),
            (Some(b), Some(f)) if &b == f => {}
            _ => {
                return Err(syn::Error::new(
                    attr_span,
                    format!(
                        "cannot infer the boundary of frame `{}`: every alternative must end in \
                         the same string literal; otherwise say which it is: `#[frame = \"\\n\"]`",
                        rule.name
                    ),
                ))
            }
        }
    }
    found.ok_or_else(|| {
        syn::Error::new(
            attr_span,
            format!(
                "frame `{}` has no alternatives to infer a boundary from",
                rule.name
            ),
        )
    })
}

/// The last element of every alternative of a frame rule must *be* the
/// boundary: the literal, `line_ending` for a newline boundary, `eof`, or a
/// group of those. That is what makes every frame end exactly at a boundary,
/// so that the piece that finds a frame's end is the one that owns it.
fn check_terminators(rule: &Rule, boundary: &str, user_rules: &HashSet<String>) -> syn::Result<()> {
    for v in &rule.variants {
        let Some(last) = v.pattern.last() else {
            return Err(syn::Error::new(
                rule.name.span(),
                format!(
                    "frame `{}` has an empty alternative; a frame must end in its boundary",
                    rule.name
                ),
            ));
        };
        if !is_terminator(last, boundary, user_rules) {
            return Err(syn::Error::new(
                last.span(),
                format!(
                    "frame `{}` must end in its boundary {:?} (the literal, `line_ending`, `eof`, \
                     or a group of those); this alternative ends in something else",
                    rule.name, boundary
                ),
            ));
        }
    }
    Ok(())
}

fn is_terminator(p: &ModelPattern, boundary: &str, user_rules: &HashSet<String>) -> bool {
    match p {
        ModelPattern::Lit { .. } => literal_text(p).as_deref() == Some(boundary),
        ModelPattern::RuleCall {
            rule_path,
            generics,
            args,
            ..
        } if generics.is_empty() && args.is_empty() => {
            let name = rule_path.segments.last().map(|s| s.ident.to_string());
            match name.as_deref() {
                Some(n) if user_rules.contains(n) => false,
                Some("eof") => true,
                Some("line_ending") => boundary == "\n" || boundary == "\r\n",
                _ => false,
            }
        }
        ModelPattern::Group { alts, .. } => alts
            .iter()
            .all(|(seq, _, _)| seq.len() == 1 && is_terminator(&seq[0], boundary, user_rules)),
        _ => false,
    }
}

fn literal_text(p: &ModelPattern) -> Option<String> {
    match p {
        ModelPattern::Lit {
            lit: syn::Lit::Str(s),
            ..
        } => Some(s.value()),
        ModelPattern::Lit {
            lit: syn::Lit::Char(c),
            ..
        } => Some(c.value().to_string()),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// The per-rule check
// -----------------------------------------------------------------------------

struct Cx<'a> {
    frame: &'a str,
    boundary: &'a str,
    user_rules: &'a HashSet<String>,
    builtins: &'a HashSet<String>,
    has_user_ws: bool,
}

impl Cx<'_> {
    fn err(&self, span: proc_macro2::Span, what: String) -> syn::Error {
        syn::Error::new(
            span,
            format!(
                "{what} can consume the boundary {:?} of frame `{}`; cutting the input at the \
                 next boundary would then land inside a frame",
                self.boundary, self.frame
            ),
        )
    }

    /// Would a literal of this text overlap the boundary? For a one-character
    /// boundary that is containment. For a longer one, any shared character
    /// is treated as overlap - a literal ending in the boundary's first half
    /// could complete it together with what follows.
    fn literal_overlaps(&self, text: &str) -> bool {
        if self.boundary.chars().count() == 1 {
            text.contains(self.boundary)
        } else {
            text.chars().any(|c| self.boundary.contains(c))
        }
    }

    fn builtin_overlaps(&self, name: &str) -> bool {
        self.boundary.chars().any(|c| builtin_may_consume(name, c))
    }
}

fn check_rule(rule: &Rule, is_frame: bool, cx: &Cx) -> syn::Result<()> {
    for v in &rule.variants {
        let interior = if is_frame {
            // The trailing element is the boundary itself (checked above).
            &v.pattern[..v.pattern.len() - 1]
        } else {
            &v.pattern[..]
        };
        check_sequence(interior, rule.is_lexical, rule, cx)?;
    }
    Ok(())
}

fn check_sequence(seq: &[ModelPattern], lexical: bool, rule: &Rule, cx: &Cx) -> syn::Result<()> {
    if !lexical && !cx.has_user_ws && seq.len() > 1 && cx.builtin_overlaps("multispace0") {
        return Err(syn::Error::new(
            seq[0].span(),
            format!(
                "rule `{}` is syntactic (lowercase), so the implicit whitespace between its \
                 elements can consume the boundary {:?} of frame `{}`; make the rule lexical \
                 (uppercase, or wrap the sequence in `lex(…)`), or define `WS` so it cannot",
                rule.name, cx.boundary, cx.frame
            ),
        ));
    }
    for p in seq {
        check_pattern(p, lexical, rule, cx)?;
    }
    Ok(())
}

fn check_pattern(p: &ModelPattern, lexical: bool, rule: &Rule, cx: &Cx) -> syn::Result<()> {
    match p {
        ModelPattern::Cut(_) | ModelPattern::Fail { .. } => Ok(()),
        // Lookahead consumes nothing.
        ModelPattern::Peek(_, _) | ModelPattern::Not(_, _) => Ok(()),
        ModelPattern::Lit { .. } => {
            if let Some(text) = literal_text(p) {
                if cx.literal_overlaps(&text) {
                    return Err(cx.err(
                        p.span(),
                        format!("the literal {text:?} in rule `{}`", rule.name),
                    ));
                }
            }
            Ok(())
        }
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            let name = rule_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            for a in args {
                let pat = match a {
                    Argument::Positional(p) | Argument::Named(_, p) => p,
                };
                check_pattern(pat, lexical, rule, cx)?;
            }
            if cx.user_rules.contains(&name) {
                // Reached; checked as a rule of its own.
                return Ok(());
            }
            if cx.builtins.contains(&name) && cx.builtin_overlaps(&name) {
                return Err(cx.err(
                    p.span(),
                    format!("the built-in `{name}` in rule `{}`", rule.name),
                ));
            }
            Ok(())
        }
        // Bounded by the code generator: the scan stops at the terminator or
        // the boundary, whichever is first. The terminator itself must still
        // not be the boundary's superset; it is checked like any pattern.
        ModelPattern::Until { pattern, .. } => check_pattern(pattern, lexical, rule, cx),
        ModelPattern::Recover { body, sync, .. } => {
            check_pattern(body, lexical, rule, cx)?;
            check_pattern(sync, lexical, rule, cx)
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                check_sequence(seq, lexical, rule, cx)?;
            }
            Ok(())
        }
        ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _)
        | ModelPattern::Parenthesized(inner, _) => {
            let delims = match p {
                ModelPattern::Bracketed(..) => "[]",
                ModelPattern::Braced(..) => "{}",
                _ => "()",
            };
            if cx.literal_overlaps(delims) {
                return Err(cx.err(
                    p.span(),
                    format!("the delimiters `{delims}` in rule `{}`", rule.name),
                ));
            }
            check_sequence(inner, lexical, rule, cx)
        }
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::SpanBinding(inner, _, _) => {
            // A repetition in a syntactic context skips whitespace before each
            // element, which is the same concern as a sequence of two.
            if matches!(p, ModelPattern::Repeat(..) | ModelPattern::Plus(..)) {
                check_sequence(std::slice::from_ref(&**inner), lexical, rule, cx)?;
                check_sequence(&[(**inner).clone(), (**inner).clone()], lexical, rule, cx)
            } else {
                check_pattern(inner, lexical, rule, cx)
            }
        }
        ModelPattern::Bounded { pattern, .. }
        | ModelPattern::Count { pattern, .. }
        | ModelPattern::Fold { pattern, .. } => check_sequence(
            &[(**pattern).clone(), (**pattern).clone()],
            lexical,
            rule,
            cx,
        ),
        ModelPattern::LexicalScope(inner, _) => check_pattern(inner, true, rule, cx),
        ModelPattern::SpacedScope(inner, _) => check_pattern(inner, false, rule, cx),
    }
}

/// Can the built-in ever consume this character? Conservative: an unknown
/// built-in is assumed to consume anything.
fn builtin_may_consume(name: &str, c: char) -> bool {
    match name {
        "eof" | "empty" => false,
        "digit" | "digit1" => c.is_ascii_digit(),
        "hex_digit0" | "hex_digit1" => c.is_ascii_hexdigit(),
        "oct_digit0" | "oct_digit1" => ('0'..='7').contains(&c),
        "binary_digit0" | "binary_digit1" => c == '0' || c == '1',
        "alpha1" => c.is_alphabetic(),
        "ident" | "raw_ident" => c.is_alphanumeric() || c == '_',
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" => c.is_ascii_digit() || c == '-' || c == '+' || c == '_',
        "f32" | "f64" => c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.' | '_'),
        "bool" => c.is_ascii_alphabetic(),
        "space0" | "space1" => c == ' ' || c == '\t',
        "multispace0" | "multispace1" => c.is_whitespace(),
        "line_ending" => c == '\n' || c == '\r',
        // `string`, `char`, `any`, and anything not listed.
        _ => true,
    }
}

// -----------------------------------------------------------------------------
// Reachability
// -----------------------------------------------------------------------------

fn collect_calls(seq: &[ModelPattern], user_rules: &HashSet<String>, out: &mut Vec<String>) {
    for p in seq {
        collect_calls_in(p, user_rules, out);
    }
}

fn collect_calls_in(p: &ModelPattern, user_rules: &HashSet<String>, out: &mut Vec<String>) {
    match p {
        ModelPattern::Cut(_) | ModelPattern::Fail { .. } | ModelPattern::Lit { .. } => {}
        ModelPattern::RuleCall {
            rule_path, args, ..
        } => {
            if let Some(seg) = rule_path.segments.last() {
                let name = seg.ident.to_string();
                if user_rules.contains(&name) {
                    out.push(name);
                }
            }
            for a in args {
                match a {
                    Argument::Positional(p) | Argument::Named(_, p) => {
                        collect_calls_in(p, user_rules, out)
                    }
                }
            }
        }
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                collect_calls(seq, user_rules, out);
            }
        }
        ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _)
        | ModelPattern::Parenthesized(inner, _) => collect_calls(inner, user_rules, out),
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _)
        | ModelPattern::LexicalScope(inner, _)
        | ModelPattern::SpacedScope(inner, _) => collect_calls_in(inner, user_rules, out),
        ModelPattern::Until { pattern, .. }
        | ModelPattern::Count { pattern, .. }
        | ModelPattern::Fold { pattern, .. }
        | ModelPattern::Bounded { pattern, .. } => collect_calls_in(pattern, user_rules, out),
        ModelPattern::Recover { body, sync, .. } => {
            collect_calls_in(body, user_rules, out);
            collect_calls_in(sync, user_rules, out);
        }
    }
}

// -----------------------------------------------------------------------------
// par_fold
// -----------------------------------------------------------------------------

/// A `par_fold` must be the whole body of its rule - one alternative, one
/// element - and fold over a frame rule. Anything before or after it in the
/// sequence would have to be parsed by every piece, or by none.
fn check_par_fold(
    rule: &Rule,
    frames: &BTreeMap<String, String>,
    par_folds: &mut BTreeMap<String, String>,
) -> syn::Result<()> {
    // Is there a par_fold anywhere in this rule?
    let mut found: Vec<(&ModelPattern, proc_macro2::Span)> = Vec::new();
    for v in &rule.variants {
        for p in &v.pattern {
            find_par_folds(p, &mut found);
        }
    }
    let Some((item, span)) = found.first().copied() else {
        return Ok(());
    };

    let sole = rule.variants.len() == 1
        && rule.variants[0].pattern.len() == 1
        && matches!(
            &rule.variants[0].pattern[0],
            ModelPattern::Fold { merge: Some(_), .. }
        );
    if !sole || found.len() > 1 {
        return Err(syn::Error::new(
            span,
            format!(
                "`par_fold` must be the whole body of rule `{}`: one alternative, nothing before \
                 or after it - the input is cut into pieces that are each folded on their own, \
                 and a prefix or suffix would belong to none of them",
                rule.name
            ),
        ));
    }

    let item_name = match item {
        ModelPattern::RuleCall { rule_path, .. } => {
            rule_path.segments.last().map(|s| s.ident.to_string())
        }
        _ => None,
    };
    match item_name {
        Some(name) if frames.contains_key(&name) => {
            par_folds.insert(rule.name.to_string(), name);
            Ok(())
        }
        Some(name) => Err(syn::Error::new(
            item.span(),
            format!(
                "`par_fold` folds over `{name}`, which is not a frame; mark it `#[frame]` so the \
                 input can be cut at its boundary"
            ),
        )),
        None => Err(syn::Error::new(
            item.span(),
            "`par_fold` folds over a rule, not an inline pattern: name a `#[frame]` rule here",
        )),
    }
}

fn find_par_folds<'a>(p: &'a ModelPattern, out: &mut Vec<(&'a ModelPattern, proc_macro2::Span)>) {
    if let ModelPattern::Fold {
        pattern,
        merge: Some(_),
        span,
        ..
    } = p
    {
        out.push((pattern, *span));
    }
    // A par_fold nested anywhere else is caught by `sole` above; the walk
    // only needs to find it.
    match p {
        ModelPattern::Group { alts, .. } => {
            for (seq, _, _) in alts {
                for q in seq {
                    find_par_folds(q, out);
                }
            }
        }
        ModelPattern::Bracketed(inner, _)
        | ModelPattern::Braced(inner, _)
        | ModelPattern::Parenthesized(inner, _) => {
            for q in inner {
                find_par_folds(q, out);
            }
        }
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _)
        | ModelPattern::SpanBinding(inner, _, _)
        | ModelPattern::Peek(inner, _)
        | ModelPattern::Not(inner, _)
        | ModelPattern::LexicalScope(inner, _)
        | ModelPattern::SpacedScope(inner, _) => find_par_folds(inner, out),
        ModelPattern::Until { pattern, .. }
        | ModelPattern::Count { pattern, .. }
        | ModelPattern::Bounded { pattern, .. } => find_par_folds(pattern, out),
        ModelPattern::Fold {
            pattern,
            merge: None,
            ..
        } => find_par_folds(pattern, out),
        ModelPattern::Recover { body, sync, .. } => {
            find_par_folds(body, out);
            find_par_folds(sync, out);
        }
        _ => {}
    }
}
