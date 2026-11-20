//! **Which byte an alternative can begin with**, so that the fast pass need
//! not try one that cannot match.
//!
//! A rule of several alternatives tries them in order, and each one that does
//! not apply costs a call, a whitespace skip and a failed comparison before the
//! next is tried. On a grammar with a keyword list or a twenty-way primary
//! expression that is most of the work: measured on Nikaia's own grammar, a
//! name was checked against some thirty keywords one call at a time. Knowing
//! the bytes an alternative can begin with turns each of those into one bit
//! test.
//!
//! **Sound or nothing.** A set is given only where every match of the
//! alternative consumes at least one byte and that byte is in the set; where
//! that cannot be shown - something that can match nothing, a builtin whose
//! first byte is anything, a hand-written parser, generics, a cycle - there is
//! no set and the alternative is tried as before.
//!
//! **Parameters.** A value parameter decides nothing about the first byte but
//! through a guard or a computed bound, which the analysis already reads as
//! "consumes nothing" and "may match nothing": so a rule called with values
//! has the set its patterns give. A pattern parameter (`p: Rule<T>`) is what
//! the call passes: the callee is read with the argument's start in its place,
//! afresh for each call. A name that is a parameter anywhere in the grammar
//! and also a rule is not resolved: inside the rule that declares it, it is
//! the parameter. A cut before
//! the first consuming element also means no set: there the alternative's
//! failure is a commitment, not a backtrack, and skipping it would turn one
//! into the other.
//!
//! Only the fast pass consults the sets (see `rt::first_byte`): an
//! alternative that was not tried contributes nothing to `expected one of: …`,
//! so the diagnosing pass tries them all, as it always has.

use crate::model::{Argument, GrammarDefinition, ModelPattern, Rule};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// A set of bytes, one bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ByteSet(pub [u64; 4]);

impl ByteSet {
    pub fn insert(&mut self, b: u8) {
        self.0[(b >> 6) as usize] |= 1 << (b & 63);
    }

    pub fn insert_range(&mut self, lo: u8, hi: u8) {
        for b in lo..=hi {
            self.insert(b);
        }
    }

    pub fn union(&mut self, other: &ByteSet) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a |= b;
        }
    }

    pub fn contains(&self, b: u8) -> bool {
        self.0[(b >> 6) as usize] & (1 << (b & 63)) != 0
    }

    /// Every byte: a guard on this set would never refuse anything.
    pub fn is_full(&self) -> bool {
        self.0.iter().all(|w| *w == u64::MAX)
    }

    fn of(lo_hi: &[(u8, u8)]) -> ByteSet {
        let mut set = ByteSet::default();
        for &(lo, hi) in lo_hi {
            set.insert_range(lo, hi);
        }
        set
    }
}

/// What one element of a sequence says about the sequence's first byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Start {
    /// Every match consumes a byte, and it is one of these.
    Consumes(ByteSet),
    /// It may match nothing; if it consumes, the first byte is one of these.
    Maybe(ByteSet),
    /// It consumes nothing - a lookahead - and says nothing either.
    Transparent,
    /// Nothing can be said.
    Unknown,
}

/// The first-byte sets of a grammar's alternatives.
pub struct FirstBytes<'g> {
    rules: HashMap<String, &'g Rule>,
    /// A rule's start, by name and by whether it is called where trivia has
    /// already been skipped: the least fixed point over the grammar, so
    /// that a recursive rule - a node inside a node - has the set its
    /// alternatives give rather than none.
    table: RefCell<HashMap<(String, bool), Start>>,
    /// The calls with patterns being read, so that a cycle through them is
    /// not followed.
    visiting: RefCell<HashSet<String>>,
    /// Every parameter's name, of every rule.
    params: HashSet<String>,
}

/// The pattern parameters of the rule being read, by name: what the call
/// passed, as a start.
type Env = HashMap<String, Start>;

impl<'g> FirstBytes<'g> {
    pub fn new(grammar: &'g GrammarDefinition) -> Self {
        let first = FirstBytes {
            rules: grammar
                .rules
                .iter()
                .map(|r| (r.name.to_string(), r))
                .collect(),
            table: RefCell::new(HashMap::new()),
            visiting: RefCell::new(HashSet::new()),
            params: grammar
                .rules
                .iter()
                .flat_map(|r| r.params.iter().map(|p| p.name.to_string()))
                .collect(),
        };
        first.solve();
        first
    }

    /// Every rule's start from nothing (no match yet) up to the fixed
    /// point: each round reads the rules called through the last round's
    /// table, and a start only grows - a larger set, `Consumes` to `Maybe`,
    /// anything to `Unknown` - so the rounds end.
    fn solve(&self) {
        let keys: Vec<(String, bool)> = self
            .rules
            .keys()
            .flat_map(|n| [(n.clone(), false), (n.clone(), true)])
            .collect();
        {
            let mut table = self.table.borrow_mut();
            for k in &keys {
                table.insert(k.clone(), Start::Consumes(ByteSet::default()));
            }
        }
        loop {
            let mut changed = false;
            for (name, trivia_skipped) in &keys {
                let rule = self.rules[name];
                let now = if !rule.is_lexical && !trivia_skipped {
                    // A syntactic rule skips whitespace first; only where
                    // that has already happened does it begin at its first
                    // token.
                    Start::Unknown
                } else {
                    self.variants(rule, *trivia_skipped, &Env::new())
                };
                let key = (name.clone(), *trivia_skipped);
                if self.table.borrow()[&key] != now {
                    self.table.borrow_mut().insert(key, now);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// The bytes an alternative can begin with, or `None` if it may begin
    /// with anything or match nothing at all.
    ///
    /// `trivia_skipped`: the alternative begins where a whitespace skip has
    /// just run - a syntactic rule's alternatives, whose skip is hoisted out of
    /// them. Only there does a syntactic rule called first begin at its own
    /// first token: anywhere else its skip may consume whitespace first.
    pub fn alternative(&self, pattern: &[ModelPattern], trivia_skipped: bool) -> Option<ByteSet> {
        match self.sequence(pattern, trivia_skipped, &Env::new()) {
            Start::Consumes(set) if !set.is_full() => Some(set),
            _ => None,
        }
    }

    fn sequence(&self, pattern: &[ModelPattern], trivia_skipped: bool, env: &Env) -> Start {
        let mut maybe = ByteSet::default();
        for p in pattern {
            match self.element(p, trivia_skipped, env) {
                Start::Consumes(set) => {
                    maybe.union(&set);
                    return Start::Consumes(maybe);
                }
                Start::Maybe(set) => maybe.union(&set),
                Start::Transparent => {}
                Start::Unknown => return Start::Unknown,
            }
        }
        Start::Maybe(maybe)
    }

    fn element(&self, p: &ModelPattern, trivia_skipped: bool, env: &Env) -> Start {
        match p {
            ModelPattern::Lit { lit, .. } => match lit {
                syn::Lit::Str(s) => match s.value().as_bytes().first() {
                    Some(&b) => Start::Consumes(ByteSet::of(&[(b, b)])),
                    None => Start::Transparent,
                },
                syn::Lit::Char(c) => {
                    let mut buf = [0u8; 4];
                    let b = c.value().encode_utf8(&mut buf).as_bytes()[0];
                    Start::Consumes(ByteSet::of(&[(b, b)]))
                }
                _ => Start::Unknown,
            },
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                ..
            } => self.call(rule_path, generics, args, trivia_skipped, env),
            ModelPattern::Group { alts, .. } => {
                let mut all = ByteSet::default();
                let mut nullable = false;
                for (seq, _, _) in alts {
                    match self.sequence(seq, trivia_skipped, env) {
                        Start::Consumes(set) => all.union(&set),
                        Start::Maybe(set) => {
                            all.union(&set);
                            nullable = true;
                        }
                        Start::Transparent => nullable = true,
                        Start::Unknown => return Start::Unknown,
                    }
                }
                if nullable {
                    Start::Maybe(all)
                } else {
                    Start::Consumes(all)
                }
            }
            ModelPattern::Parenthesized(..) => Start::Consumes(ByteSet::of(&[(b'(', b'(')])),
            ModelPattern::Bracketed(..) => Start::Consumes(ByteSet::of(&[(b'[', b'[')])),
            ModelPattern::Braced(..) => Start::Consumes(ByteSet::of(&[(b'{', b'{')])),
            ModelPattern::Optional(inner, _) | ModelPattern::Repeat(inner, _) => {
                optional(self.element(inner, trivia_skipped, env))
            }
            ModelPattern::Bounded { pattern, min, .. } if *min == 0 => {
                optional(self.element(pattern, trivia_skipped, env))
            }
            ModelPattern::Plus(inner, _) | ModelPattern::Bounded { pattern: inner, .. } => {
                self.element(inner, trivia_skipped, env)
            }
            ModelPattern::SpanBinding(inner, _, _) => self.element(inner, trivia_skipped, env),
            // `count(p)` matches `p` any number of times, none too.
            ModelPattern::Count { pattern, .. } => {
                optional(self.element(pattern, trivia_skipped, env))
            }
            // A lookahead consumes nothing. `peek(p)` where `p` begins with a
            // known byte fixes the byte here, which is the sequence's first
            // where nothing was consumed before: past it - a cut included -
            // the sequence is reached only with that byte, and at the end of
            // the input not at all. `not(..)` only adds a condition, and
            // leaving it out makes the set larger.
            ModelPattern::Peek(inner, _, _) => match self.element(inner, trivia_skipped, env) {
                Start::Consumes(set) => Start::Consumes(set),
                _ => Start::Transparent,
            },
            ModelPattern::Not(..) => Start::Transparent,
            // A guard consumes nothing either, and can only fail more.
            ModelPattern::Guard { .. } => Start::Transparent,
            // A cut before anything is consumed makes the alternative's failure
            // a commitment; `until`, `recover`, the scopes and the folds are
            // left alone.
            _ => Start::Unknown,
        }
    }

    /// A call of a rule, a parameter or a builtin.
    fn call(
        &self,
        rule_path: &syn::Path,
        generics: &[syn::Type],
        args: &[Argument],
        trivia_skipped: bool,
        env: &Env,
    ) -> Start {
        if !generics.is_empty() {
            return Start::Unknown;
        }
        let Some(name) = rule_path.get_ident().map(|i| i.to_string()) else {
            return Start::Unknown;
        };
        // A pattern parameter of the rule being read: what the call
        // passed. Any other parameter's name is not resolved.
        if let Some(start) = env.get(&name) {
            return *start;
        }
        if self.params.contains(&name) {
            return Start::Unknown;
        }
        // What the call passes as patterns, read where it is made.
        let patterns: Vec<(Option<String>, Start)> = args
            .iter()
            .filter_map(|a| match a {
                Argument::Positional(p) => Some((None, self.element(p, trivia_skipped, env))),
                Argument::Named(n, p) => {
                    Some((Some(n.to_string()), self.element(p, trivia_skipped, env)))
                }
                Argument::Value(..) => None,
            })
            .collect();
        // The grammar's own rule under a builtin's name is that rule.
        if self.rules.contains_key(&name) {
            if patterns.is_empty() {
                return self.rule(&name, trivia_skipped);
            }
            return self.rule_with(&name, trivia_skipped, &patterns);
        }
        match (name.as_str(), patterns.as_slice()) {
            ("empty", []) => Start::Transparent,
            // `eof` consumes nothing and matches only where there
            // is no byte: a sequence it makes nullable is not
            // guarded, and after it nothing more can match.
            ("eof", []) => Start::Transparent,
            // What `p` matched, as text, bytes or a span: `p`'s start.
            ("text" | "bytes" | "span", [(None, p)]) => *p,
            // `any` consumes one byte, whichever it is.
            ("any", []) => Start::Consumes(ByteSet([u64::MAX; 4])),
            (_, []) if args.is_empty() => builtin(&name),
            _ => Start::Unknown,
        }
    }

    /// A rule called with values only: its start, the same at every call.
    fn rule(&self, name: &str, trivia_skipped: bool) -> Start {
        self.table.borrow()[&(name.to_string(), trivia_skipped)]
    }

    /// A rule called with patterns: read afresh with them in place of its
    /// pattern parameters (positional ones by position, named ones by name).
    fn rule_with(
        &self,
        name: &str,
        trivia_skipped: bool,
        patterns: &[(Option<String>, Start)],
    ) -> Start {
        let rule = self.rules[name];
        if !rule.is_lexical && !trivia_skipped {
            return Start::Unknown;
        }
        let mut env = Env::new();
        for (i, (named, start)) in patterns.iter().enumerate() {
            let param = match named {
                Some(n) => rule.params.iter().find(|p| p.name == n.as_str()),
                None => rule.params.get(i),
            };
            match param {
                Some(p) => {
                    env.insert(p.name.to_string(), *start);
                }
                None => return Start::Unknown,
            }
        }
        // A cycle through calls with patterns is not followed.
        let key = name.to_string();
        if !self.visiting.borrow_mut().insert(key.clone()) {
            return Start::Unknown;
        }
        let result = self.variants(rule, trivia_skipped, &env);
        self.visiting.borrow_mut().remove(&key);
        result
    }

    /// What a rule's alternatives together begin with.
    fn variants(&self, rule: &Rule, trivia_skipped: bool, env: &Env) -> Start {
        if !rule.generics.params.is_empty() && env.is_empty() {
            return Start::Unknown;
        }
        // Each alternative of a syntactic rule begins after its skip, whether
        // that is hoisted or made inside a single alternative.
        let inner = trivia_skipped || !rule.is_lexical;
        let mut all = ByteSet::default();
        let mut nullable = false;
        for v in &rule.variants {
            match self.sequence(&v.pattern, inner, env) {
                Start::Consumes(set) => all.union(&set),
                Start::Maybe(set) => {
                    all.union(&set);
                    nullable = true;
                }
                Start::Transparent => nullable = true,
                Start::Unknown => return Start::Unknown,
            }
        }
        if nullable {
            Start::Maybe(all)
        } else {
            Start::Consumes(all)
        }
    }
}

fn optional(inner: Start) -> Start {
    match inner {
        Start::Consumes(set) | Start::Maybe(set) => Start::Maybe(set),
        other => other,
    }
}

/// The builtins whose first byte is known. Every other one - `any`, `eof`,
/// the `*0` runs, the numbers with a sign - says nothing.
fn builtin(name: &str) -> Start {
    // `raw_ident` is the ASCII class or a wide alphanumeric character, whose
    // first byte is a UTF-8 lead byte.
    let set = match name {
        "ident" | "raw_ident" => ByteSet::of(&[
            (b'0', b'9'),
            (b'a', b'z'),
            (b'A', b'Z'),
            (b'_', b'_'),
            (0x80, 0xFF),
        ]),
        "digit" | "digit1" => ByteSet::of(&[(b'0', b'9')]),
        "alpha1" => ByteSet::of(&[(b'a', b'z'), (b'A', b'Z')]),
        "hex_digit" | "hex_digit1" => ByteSet::of(&[(b'0', b'9'), (b'a', b'f'), (b'A', b'F')]),
        "oct_digit1" => ByteSet::of(&[(b'0', b'7')]),
        "binary_digit1" => ByteSet::of(&[(b'0', b'1')]),
        "space1" => ByteSet::of(&[(b'\t', b'\t'), (b' ', b' ')]),
        "multispace1" => ByteSet::of(&[(b'\t', b'\r'), (b' ', b' ')]),
        "string" => ByteSet::of(&[(b'"', b'"')]),
        "char" => ByteSet::of(&[(b'\'', b'\'')]),
        _ => return Start::Unknown,
    };
    Start::Consumes(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn grammar(input: proc_macro2::TokenStream) -> GrammarDefinition {
        let parsed: crate::parser::GrammarDefinition = syn::parse2(input).unwrap();
        parsed.into()
    }

    /// The sets of a rule's alternatives, as the generator asks for them.
    fn sets(g: &GrammarDefinition, rule: &str) -> Vec<Option<Vec<u8>>> {
        let first = FirstBytes::new(g);
        let rule = g.rules.iter().find(|r| r.name == rule).unwrap();
        rule.variants
            .iter()
            .map(|v| {
                first
                    .alternative(&v.pattern, !rule.is_lexical)
                    .map(|set| (0..=255u8).filter(|b| set.contains(*b)).collect())
            })
            .collect()
    }

    #[test]
    fn a_keyword_list_is_its_first_letters() {
        let g = grammar(quote! {
            grammar test {
                rule RESERVED = KW_AS | KW_FN | "if" not(ident)
                rule KW_AS = "as" not(ident)
                rule KW_FN = "fn" not(ident)
            }
        });
        assert_eq!(
            sets(&g, "RESERVED"),
            vec![Some(vec![b'a']), Some(vec![b'f']), Some(vec![b'i'])]
        );
    }

    #[test]
    fn what_can_match_nothing_or_anything_has_no_set() {
        let g = grammar(quote! {
            grammar test {
                rule A = "x"? "y" | "z"* | any | until(";") | "" | ident
            }
        });
        let ident: Vec<u8> = (0..=255u8)
            .filter(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80)
            .collect();
        assert_eq!(
            sets(&g, "A"),
            vec![Some(vec![b'x', b'y']), None, None, None, None, Some(ident)]
        );
    }

    #[test]
    fn a_syntactic_rule_counts_only_after_the_skip() {
        let g = grammar(quote! {
            grammar test {
                rule item = call | "x"
                rule call = "f" "(" ")"
                rule LEX = call | "x"
            }
        });
        // In a syntactic rule the skip is hoisted, so `call` begins at `f`;
        // in a lexical one, `call`'s own skip may consume a blank first.
        assert_eq!(sets(&g, "item"), vec![Some(vec![b'f']), Some(vec![b'x'])]);
        assert_eq!(sets(&g, "LEX"), vec![None, Some(vec![b'x'])]);
    }

    #[test]
    fn a_cut_before_the_first_byte_has_no_set_and_a_cycle_its_bases() {
        let g = grammar(quote! {
            grammar test {
                rule A = => "x" | "y" => "z" | B
                rule B = B "b" | "c"
            }
        });
        // Every match of `B` begins with its base case's `c`: the fixed
        // point finds it where reading `B` inside `B` gave up.
        assert_eq!(
            sets(&g, "A"),
            vec![None, Some(vec![b'y']), Some(vec![b'c'])]
        );
    }

    #[test]
    fn a_rule_called_with_values_has_the_set_its_patterns_give() {
        let g = grammar(quote! {
            grammar test {
                rule A(n: i64) = Seq(m = n) | Here "?" Item | "z"
                rule Seq(m: i64) = if(m > 0) "-" " "* "x"
                rule Here -> usize = empty
                rule Item = "i"
            }
        });
        assert_eq!(
            sets(&g, "A"),
            vec![Some(vec![b'-']), Some(vec![b'?']), Some(vec![b'z'])]
        );
    }

    #[test]
    fn a_pattern_argument_stands_in_for_its_parameter() {
        let g = grammar(quote! {
            grammar test {
                rule A = Count(p = " ") "-" | Twice(p = "a") | Count(p = " ")
                rule Count<T>(p: Rule<T>) -> usize = n:count(p) -> { n }
                rule Twice<T>(p: Rule<T>) = p p
            }
        });
        // `count` may match nothing, so `-` counts too; on its own it gives
        // no set.
        assert_eq!(
            sets(&g, "A"),
            vec![Some(vec![b' ', b'-']), Some(vec![b'a']), None]
        );
    }

    #[test]
    fn a_parameter_named_like_a_rule_is_not_that_rule() {
        let g = grammar(quote! {
            grammar test {
                rule A = Wrap(B = "c") | B
                rule Wrap<T>(B: Rule<T>) = B
                rule B = "b"
            }
        });
        // Inside `Wrap`, `B` is the parameter; outside, a name some rule
        // declares as a parameter is not resolved at all.
        assert_eq!(sets(&g, "A"), vec![Some(vec![b'c']), None]);
    }
}
