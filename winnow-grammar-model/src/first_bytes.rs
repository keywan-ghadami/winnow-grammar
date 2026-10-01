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
//! first byte is anything, a hand-written parser, a rule with parameters, a
//! cycle - there is no set and the alternative is tried as before. A cut before
//! the first consuming element also means no set: there the alternative's
//! failure is a commitment, not a backtrack, and skipping it would turn one
//! into the other.
//!
//! Only the fast pass consults the sets (see `rt::first_byte`): an
//! alternative that was not tried contributes nothing to `expected one of: …`,
//! so the diagnosing pass tries them all, as it always has.

use crate::model::{GrammarDefinition, ModelPattern, Rule};
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
#[derive(Clone, Copy)]
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
    /// A rule's set, by name and by whether it is called where trivia has
    /// already been skipped. `None` while it is being computed, which is how a
    /// cycle comes out unknown.
    memo: RefCell<HashMap<(String, bool), Option<ByteSet>>>,
    visiting: RefCell<HashSet<(String, bool)>>,
}

impl<'g> FirstBytes<'g> {
    pub fn new(grammar: &'g GrammarDefinition) -> Self {
        FirstBytes {
            rules: grammar
                .rules
                .iter()
                .map(|r| (r.name.to_string(), r))
                .collect(),
            memo: RefCell::new(HashMap::new()),
            visiting: RefCell::new(HashSet::new()),
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
        match self.sequence(pattern, trivia_skipped) {
            Start::Consumes(set) if !set.is_full() => Some(set),
            _ => None,
        }
    }

    fn sequence(&self, pattern: &[ModelPattern], trivia_skipped: bool) -> Start {
        let mut maybe = ByteSet::default();
        for p in pattern {
            match self.element(p, trivia_skipped) {
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

    fn element(&self, p: &ModelPattern, trivia_skipped: bool) -> Start {
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
            } => {
                if !generics.is_empty() || !args.is_empty() {
                    return Start::Unknown;
                }
                let Some(name) = rule_path.get_ident().map(|i| i.to_string()) else {
                    return Start::Unknown;
                };
                // The grammar's own rule under a builtin's name is that rule.
                if self.rules.contains_key(&name) {
                    return match self.rule(&name, trivia_skipped) {
                        Some(set) => Start::Consumes(set),
                        None => Start::Unknown,
                    };
                }
                builtin(&name)
            }
            ModelPattern::Group { alts, .. } => {
                let mut all = ByteSet::default();
                let mut nullable = false;
                for (seq, _, _) in alts {
                    match self.sequence(seq, trivia_skipped) {
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
                optional(self.element(inner, trivia_skipped))
            }
            ModelPattern::Bounded { pattern, min, .. } if *min == 0 => {
                optional(self.element(pattern, trivia_skipped))
            }
            ModelPattern::Plus(inner, _) | ModelPattern::Bounded { pattern: inner, .. } => {
                self.element(inner, trivia_skipped)
            }
            ModelPattern::SpanBinding(inner, _, _) => self.element(inner, trivia_skipped),
            // A lookahead consumes nothing; what it demands of the byte is a
            // further condition, and leaving it out only makes the set larger.
            ModelPattern::Peek(..) | ModelPattern::Not(..) => Start::Transparent,
            // A cut before anything is consumed makes the alternative's failure
            // a commitment; `until`, `recover`, the scopes and the folds are
            // left alone.
            _ => Start::Unknown,
        }
    }

    fn rule(&self, name: &str, trivia_skipped: bool) -> Option<ByteSet> {
        let rule = self.rules[name];
        // A syntactic rule skips whitespace first; only where that has already
        // happened does it begin at its first token.
        if !rule.is_lexical && !trivia_skipped {
            return None;
        }
        if !rule.params.is_empty() {
            return None;
        }
        let key = (name.to_string(), trivia_skipped);
        if let Some(known) = self.memo.borrow().get(&key) {
            return *known;
        }
        if !self.visiting.borrow_mut().insert(key.clone()) {
            return None;
        }
        // Each alternative of a syntactic rule begins after its skip, whether
        // that is hoisted or made inside a single alternative.
        let inner = trivia_skipped || !rule.is_lexical;
        let mut all = ByteSet::default();
        let mut known = true;
        for v in &rule.variants {
            match self.sequence(&v.pattern, inner) {
                Start::Consumes(set) => all.union(&set),
                _ => {
                    known = false;
                    break;
                }
            }
        }
        self.visiting.borrow_mut().remove(&key);
        let result = known.then_some(all);
        self.memo.borrow_mut().insert(key, result);
        result
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
        "hex_digit1" => ByteSet::of(&[(b'0', b'9'), (b'a', b'f'), (b'A', b'F')]),
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
    fn a_cut_before_the_first_byte_or_a_cycle_has_no_set() {
        let g = grammar(quote! {
            grammar test {
                rule A = => "x" | "y" => "z" | B
                rule B = B "b" | "c"
            }
        });
        assert_eq!(sets(&g, "A"), vec![None, Some(vec![b'y']), None]);
    }
}
