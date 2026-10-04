//! A run of fixed-shape elements in a lexical rule, matched by index - ADR 24 §8.
//!
//! `"-"? digit{1,2} "." digit` is four parser calls, one of them a repetition
//! with a checkpoint per element. Its width is bounded (three to five bytes)
//! and every element is a byte test, so the same match is a few comparisons
//! on the input's bytes and one advance at the end. That is what this
//! generates, for the elements below, in a lexical rule, wherever two or more
//! of them stand next to each other.
//!
//! **Only the fast pass's accepting path is new.** When the indexed match
//! fails, the input is where it was and the elements run again as they always
//! did, so a failure - its position, a `cut` after a `=>` - comes from exactly
//! the code that produced it before. The diagnosing pass (ADR 17) does not
//! take the indexed path at all: an optional element that did not match
//! records what it expected ("also possible here"), and that record is the
//! elements' to make. The two paths agree on every input the fast one accepts
//! because each element is matched greedily in both, and a sequence does not
//! backtrack into an element that matched.

use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote_spanned};
use winnow_grammar_model::model::ModelPattern;

/// One element the indexed match understands, and the value its binding gets
/// - the same type the element's own parser yields.
enum Fixed<'p> {
    /// A string or char literal: `&'a str`.
    Lit(Vec<u8>, Option<&'p syn::Ident>),
    /// `"…"?`: `Option<&'a str>`.
    OptLit(Vec<u8>, Option<&'p syn::Ident>),
    /// The built-in `digit`: `char`.
    Digit(Option<&'p syn::Ident>),
    /// `digit?`: `Option<char>`.
    OptDigit(Option<&'p syn::Ident>),
    /// `digit{min,max}` with an upper bound: `&'a str` when bound.
    Digits(usize, usize, Option<&'p syn::Ident>),
}

impl Fixed<'_> {
    fn binding(&self) -> Option<&syn::Ident> {
        match self {
            Fixed::Lit(_, b)
            | Fixed::OptLit(_, b)
            | Fixed::Digit(b)
            | Fixed::OptDigit(b)
            | Fixed::Digits(_, _, b) => *b,
        }
    }
}

/// Longer runs of digits than this are left to the repetition: the point is
/// a bounded field, not a number of any length.
const MAX_DIGITS: usize = 16;

impl<'a> Codegen<'a> {
    fn literal_bytes(lit: &syn::Lit) -> Option<Vec<u8>> {
        match lit {
            syn::Lit::Str(s) => Some(s.value().into_bytes()),
            syn::Lit::Char(c) => Some(c.value().to_string().into_bytes()),
            _ => None,
        }
    }

    /// Is `p` the built-in `digit` (not a rule of the grammar's own by that
    /// name), and with what binding?
    fn builtin_digit<'p>(&self, p: &'p ModelPattern) -> Option<Option<&'p syn::Ident>> {
        match p {
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                binding,
                ..
            } if generics.is_empty()
                && args.is_empty()
                && rule_path.is_ident("digit")
                && !self.user_rules.contains("digit") =>
            {
                Some(binding.as_ref())
            }
            _ => None,
        }
    }

    fn fixed_element<'p>(&self, p: &'p ModelPattern) -> Option<Fixed<'p>> {
        match p {
            ModelPattern::Lit { lit, binding, .. } => {
                let bytes = Self::literal_bytes(lit)?;
                (!bytes.is_empty()).then_some(Fixed::Lit(bytes, binding.as_ref()))
            }
            ModelPattern::Optional(inner, _) => match &**inner {
                ModelPattern::Lit { lit, binding, .. } => {
                    let bytes = Self::literal_bytes(lit)?;
                    (!bytes.is_empty()).then_some(Fixed::OptLit(bytes, binding.as_ref()))
                }
                other => self.builtin_digit(other).map(Fixed::OptDigit),
            },
            ModelPattern::Bounded {
                pattern,
                min,
                max: Some(max),
                ..
            } if *max <= MAX_DIGITS && min <= max && *max > 0 => self
                .builtin_digit(pattern)
                .map(|b| Fixed::Digits(*min, *max, b)),
            other => self.builtin_digit(other).map(Fixed::Digit),
        }
    }

    /// The steps of a lexical sequence, with every run of two or more
    /// fixed-shape elements matched by index (see the module comment).
    /// `None` when there is no such run, so the caller generates the
    /// sequence as before.
    pub(super) fn fixed_sequence_steps(
        &self,
        patterns: &[ModelPattern],
        in_cut: bool,
    ) -> Option<TokenStream> {
        let shapes: Vec<Option<Fixed>> = patterns.iter().map(|p| self.fixed_element(p)).collect();
        let has_run = shapes.windows(2).any(|w| w[0].is_some() && w[1].is_some());
        if !has_run {
            return None;
        }
        let mut steps = TokenStream::new();
        let mut in_cut = in_cut;
        let mut i = 0;
        while i < patterns.len() {
            let mut j = i;
            while j < patterns.len() && shapes[j].is_some() {
                j += 1;
            }
            if j - i >= 2 {
                let run: Vec<&Fixed> = shapes[i..j].iter().map(|s| s.as_ref().unwrap()).collect();
                steps.extend(self.fixed_run(&run, &patterns[i..j], in_cut));
                i = j;
                continue;
            }
            if let ModelPattern::Cut(_) = &patterns[i] {
                in_cut = true;
            }
            steps.extend(self.generate_step(&patterns[i], in_cut, true));
            i += 1;
        }
        Some(steps)
    }

    fn fixed_run(&self, run: &[&Fixed], patterns: &[ModelPattern], in_cut: bool) -> TokenStream {
        let span = Span::mixed_site();
        let input = &self.input_ident;
        let s = format_ident!("__fixed_s", span = span);
        let b = format_ident!("__fixed_b", span = span);
        let at = format_ident!("__fixed_at", span = span);

        // What a failed match breaks out with: the run either yields its
        // bindings or, with none, only whether it matched.
        let fail = if run.iter().any(|f| f.binding().is_some()) {
            quote_spanned! {span=> ::core::option::Option::None }
        } else {
            quote_spanned! {span=> false }
        };
        let mut matches = TokenStream::new();
        let mut values = Vec::new();
        for (k, f) in run.iter().enumerate() {
            let v = format_ident!("__fixed_{}", k, span = span);
            // An element nobody named is matched and not kept.
            let v_pat = if f.binding().is_some() {
                quote_spanned! {span=> #v }
            } else {
                quote_spanned! {span=> _ }
            };
            let m = match f {
                // An element nobody named is matched and not sliced: a cut
                // of the text is two character-boundary checks (#23).
                Fixed::Lit(bytes, b_) => {
                    let n = bytes.len();
                    let lit = syn::LitByteStr::new(bytes, span);
                    let value = b_.map(|_| quote_spanned! {span=> let #v = &#s[#at..#at + #n]; });
                    quote_spanned! {span=>
                        if !#b[#at..].starts_with(#lit) { break 'fixed #fail; }
                        #value
                        #at += #n;
                    }
                }
                Fixed::OptLit(bytes, b_) if b_.is_none() => {
                    let n = bytes.len();
                    let lit = syn::LitByteStr::new(bytes, span);
                    quote_spanned! {span=>
                        if #b[#at..].starts_with(#lit) { #at += #n; }
                    }
                }
                Fixed::OptLit(bytes, _) => {
                    let n = bytes.len();
                    let lit = syn::LitByteStr::new(bytes, span);
                    quote_spanned! {span=>
                        let #v_pat = if #b[#at..].starts_with(#lit) {
                            let v = &#s[#at..#at + #n];
                            #at += #n;
                            ::core::option::Option::Some(v)
                        } else {
                            ::core::option::Option::None
                        };
                    }
                }
                Fixed::Digit(_) => quote_spanned! {span=>
                    let #v_pat = match #b.get(#at) {
                        ::core::option::Option::Some(&c) if c.is_ascii_digit() => { #at += 1; c as char }
                        _ => break 'fixed #fail,
                    };
                },
                Fixed::OptDigit(_) => quote_spanned! {span=>
                    let #v_pat = match #b.get(#at) {
                        ::core::option::Option::Some(&c) if c.is_ascii_digit() => { #at += 1; ::core::option::Option::Some(c as char) }
                        _ => ::core::option::Option::None,
                    };
                },
                Fixed::Digits(min, max, b_) => {
                    let value = b_.map(|_| quote_spanned! {span=> let #v = &#s[start..#at]; });
                    quote_spanned! {span=>
                        let start = #at;
                        while #at - start < #max && #b.get(#at).is_some_and(u8::is_ascii_digit) {
                            #at += 1;
                        }
                        if #at - start < #min { break 'fixed #fail; }
                        #value
                    }
                }
            };
            matches.extend(m);
            if let Some(name) = f.binding() {
                values.push((v, name.clone()));
            }
        }
        let names: Vec<&syn::Ident> = values.iter().map(|(_, n)| n).collect();
        let vals: Vec<&syn::Ident> = values.iter().map(|(v, _)| v).collect();
        // The fallback: the same elements, generated as they always were,
        // their bindings handed out as the indexed match's would be.
        let fallback = patterns.iter().map(|p| self.generate_step(p, in_cut, true));
        let prelude = quote_spanned! {span=>
            let #s: &'a str = ::winnow_grammar::rt::rest(#input);
            let #b = #s.as_bytes();
            let mut #at = 0usize;
        };
        // The diagnosing pass keeps the elements: an optional one that did
        // not match leaves "also possible here" behind, which the indexed
        // match has no reason to know about.
        let fast_pass = quote_spanned! {span=> !<E as ::winnow_grammar::Diagnostics>::RECORDING };
        if names.is_empty() {
            // Nothing bound: no tuple, so no `()` for Clippy to point at.
            return quote_spanned! {span=>
                {
                    #prelude
                    let found = #fast_pass && 'fixed: {
                        #matches
                        true
                    };
                    if found {
                        ::winnow_grammar::rt::advance(#input, #at);
                    } else {
                        #(#fallback)*
                    }
                }
            };
        }
        quote_spanned! {span=>
            let (#(#names,)*) = {
                #prelude
                let found = if #fast_pass {
                    'fixed: {
                        #matches
                        ::core::option::Option::Some((#(#vals,)*))
                    }
                } else {
                    ::core::option::Option::None
                };
                match found {
                    ::core::option::Option::Some(v) => {
                        ::winnow_grammar::rt::advance(#input, #at);
                        v
                    }
                    ::core::option::Option::None => {
                        #(#fallback)*
                        (#(#names,)*)
                    }
                }
            };
        }
    }
}
