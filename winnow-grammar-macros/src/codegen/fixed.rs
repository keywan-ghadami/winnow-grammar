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
//! the code that produced it before. `dec<T>(digit{m,n})` joins a run when
//! `n` digits always fit in `T` (ADR 24 §8c): the value is accumulated as the
//! digits are matched, and a wider bound keeps `rt::dec` and its overflow
//! error. The diagnosing pass (ADR 17) does not
//! take the indexed path at all: an optional element that did not match
//! records what it expected ("also possible here"), and that record is the
//! elements' to make. The two paths agree on every input the fast one accepts
//! because each element is matched greedily in both, and a sequence does not
//! backtrack into an element that matched.

use super::Codegen;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote_spanned};
use winnow_grammar_model::model::{Argument, ModelPattern};

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
    /// `dec<T>(digit{min,max})` where `max` digits always fit in `T`: `T`.
    Dec(&'p syn::Type, usize, usize, Option<&'p syn::Ident>),
}

impl Fixed<'_> {
    fn binding(&self) -> Option<&syn::Ident> {
        match self {
            Fixed::Lit(_, b)
            | Fixed::OptLit(_, b)
            | Fixed::Digit(b)
            | Fixed::OptDigit(b)
            | Fixed::Digits(_, _, b)
            | Fixed::Dec(_, _, _, b) => *b,
        }
    }
}

/// Longer runs of digits than this are left to the repetition: the point is
/// a bounded field, not a number of any length.
const MAX_DIGITS: usize = 16;

/// How many decimal digits always fit in the integer type `ty`: one fewer
/// than its largest value has. `None` for anything else, `usize` and `isize`
/// included - their width is the target's, which a proc macro does not know.
fn digits_that_fit(ty: &syn::Type) -> Option<usize> {
    let syn::Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let name = p.path.get_ident()?.to_string();
    Some(match name.as_str() {
        "u8" | "i8" => 2,
        "u16" | "i16" => 4,
        "u32" | "i32" => 9,
        "u64" => 19,
        "i64" => 18,
        "u128" | "i128" => 38,
        _ => return None,
    })
}

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
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                binding,
            } if rule_path.is_ident("dec") && !self.user_rules.contains("dec") => {
                let ([ty], [Argument::Positional(arg)]) = (&generics[..], &args[..]) else {
                    return None;
                };
                let ModelPattern::Bounded {
                    pattern,
                    min,
                    max: Some(max),
                    ..
                } = arg
                else {
                    return None;
                };
                self.builtin_digit(pattern)?;
                (min <= max && *max > 0 && *max <= digits_that_fit(ty)?).then_some(Fixed::Dec(
                    ty,
                    (*min).max(1),
                    *max,
                    binding.as_ref(),
                ))
            }
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

    /// The text of the one ASCII byte at `at` of the run's slice `s`: a
    /// checked slice over bytes, `rt::ascii_at` over text, whose check the
    /// compare just made answers.
    fn byte_text(&self, s: &syn::Ident, at: &syn::Ident) -> TokenStream {
        let span = Span::mixed_site();
        if self.is_bytes() {
            quote_spanned! {span=> &#s[#at..#at + 1] }
        } else {
            quote_spanned! {span=> ::winnow_grammar::rt::ascii_at(#s, #at) }
        }
    }

    fn fixed_run(&self, run: &[&Fixed], patterns: &[ModelPattern], in_cut: bool) -> TokenStream {
        let rts = self.rts();
        let span = Span::mixed_site();
        let input = &self.input_ident;
        let s = format_ident!("__fixed_s", span = span);
        let b = format_ident!("__fixed_b", span = span);
        let at = format_ident!("__fixed_at", span = span);

        // What a failed match breaks out with: the run either yields its
        // bindings or, with none, only whether it matched.
        let slice_of_rest = if self.is_bytes() {
            quote_spanned! {span=>
                let #s: &'a [u8] = #rts::rest(#input);
                let #b = #s;
            }
        } else {
            quote_spanned! {span=>
                let #s: &'a str = #rts::rest(#input);
                let #b = #s.as_bytes();
            }
        };
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
                // One ASCII byte, the usual separator or sign: compared as a
                // byte, its text cut where the compare shows a boundary, and
                // an optional one taken without a branch on whether it is
                // there - a sign is as often there as not.
                Fixed::Lit(bytes, b_) if bytes.len() == 1 => {
                    let byte = bytes[0];
                    let value = b_.map(|_| {
                        let cut = self.byte_text(&s, &at);
                        quote_spanned! {span=> let #v = #cut; }
                    });
                    quote_spanned! {span=>
                        if #b.get(#at) != ::core::option::Option::Some(&#byte) { break 'fixed #fail; }
                        #value
                        #at += 1;
                    }
                }
                Fixed::OptLit(bytes, b_) if bytes.len() == 1 => {
                    let byte = bytes[0];
                    let value = b_.map(|_| {
                        let cut = self.byte_text(&s, &at);
                        quote_spanned! {span=>
                            let #v = if hit { ::core::option::Option::Some(#cut) } else { ::core::option::Option::None };
                        }
                    });
                    quote_spanned! {span=>
                        let hit = #b.get(#at) == ::core::option::Option::Some(&#byte);
                        #value
                        #at += usize::from(hit);
                    }
                }
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
                    // A run of digits is text in both kinds of grammar; over
                    // bytes the class makes it so (ADR 25 D2).
                    let value = b_.map(|_| {
                        if self.is_bytes() {
                            quote_spanned! {span=> let #v = ::winnow_grammar::rt::bytes::class_text(::winnow_grammar::ascii::AsciiClass::DIGIT, &#s[start..#at]); }
                        } else {
                            quote_spanned! {span=> let #v = &#s[start..#at]; }
                        }
                    });
                    quote_spanned! {span=>
                        let start = #at;
                        while #at - start < #max && #b.get(#at).is_some_and(u8::is_ascii_digit) {
                            #at += 1;
                        }
                        if #at - start < #min { break 'fixed #fail; }
                        #value
                    }
                }
                // The value as the digits are matched: `max` of them always
                // fit, so the wrapping operations never wrap, and the result
                // is the one `str::parse` gives. No digit at all is `dec`'s
                // error, left to the elements (`min` is at least one), and
                // `max` is at most 38, the most any type takes.
                Fixed::Dec(ty, min, max, _) => {
                    // One test per digit, written out: a short field is a
                    // few compares, not a loop with a counter.
                    let mut digits = TokenStream::new();
                    for k in 0..*max {
                        let step = if k == 0 {
                            quote_spanned! {span=> value = (c - b'0') as #ty; }
                        } else {
                            quote_spanned! {span=> value = value.wrapping_mul(10).wrapping_add((c - b'0') as #ty); }
                        };
                        let missing = if k < *min {
                            quote_spanned! {span=> break 'fixed #fail }
                        } else {
                            quote_spanned! {span=> break 'dec }
                        };
                        digits.extend(quote_spanned! {span=>
                            match #b.get(#at) {
                                ::core::option::Option::Some(&c) if c.is_ascii_digit() => {
                                    #step
                                    #at += 1;
                                }
                                _ => #missing,
                            }
                        });
                    }
                    quote_spanned! {span=>
                        let mut value: #ty = 0;
                        #[allow(unused_labels)]
                        'dec: {
                            #digits
                        }
                        let #v_pat = value;
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
            #slice_of_rest
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
                        #rts::advance(#input, #at);
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
                        #rts::advance(#input, #at);
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
