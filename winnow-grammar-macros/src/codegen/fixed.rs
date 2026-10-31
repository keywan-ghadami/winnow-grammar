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
//! the code that produced it before. `hex_digit` is matched like `digit`, by
//! its byte, alone, optional or in a bounded run (`hex_digit{2}`).
//! `dec<T>(digit{m,n})` joins a run when
//! `n` digits always fit in `T` (ADR 24 §8c): the value is accumulated as the
//! digits are matched, and a wider bound keeps `rt::dec` and its overflow
//! error. The diagnosing pass (ADR 17) does not
//! take the indexed path at all: an optional element that did not match
//! records what it expected ("also possible here"), and that record is the
//! elements' to make. The two paths agree on every input the fast one accepts
//! because each element is matched greedily in both, and a sequence does not
//! backtrack into an element that matched.
//!
//! A call of a rule whose whole body is such a run is an element too (ADR 24
//! §8d, `inlined_rule`): its elements stand in the call's place and its action
//! runs on what they bound once the run matched. The rule keeps its own
//! parser, and the fallback calls it, so its errors name it as before.

use super::Codegen;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use winnow_grammar_model::model::{Argument, ModelPattern, Rule};

/// The one-byte digit classes the indexed match tests: the built-ins `digit`
/// and `hex_digit`.
#[derive(Clone, Copy)]
enum DigitClass {
    Dec,
    Hex,
}

impl DigitClass {
    /// The built-in that is one digit of the class.
    fn builtin(self) -> &'static str {
        match self {
            DigitClass::Dec => "digit",
            DigitClass::Hex => "hex_digit",
        }
    }

    /// `u8`'s test for the class, a path usable as `fn(&u8) -> bool`.
    fn test(self) -> TokenStream {
        let span = Span::mixed_site();
        match self {
            DigitClass::Dec => quote_spanned! {span=> u8::is_ascii_digit },
            DigitClass::Hex => quote_spanned! {span=> u8::is_ascii_hexdigit },
        }
    }

    /// The `AsciiClass` that makes a run of the class text over bytes.
    fn ascii_class(self) -> TokenStream {
        let span = Span::mixed_site();
        match self {
            DigitClass::Dec => quote_spanned! {span=> ::winnow_grammar::ascii::AsciiClass::DIGIT },
            DigitClass::Hex => {
                quote_spanned! {span=> ::winnow_grammar::ascii::AsciiClass::HEX_DIGIT }
            }
        }
    }
}

/// One element the indexed match understands, and the value its binding gets
/// - the same type the element's own parser yields.
enum Fixed<'p> {
    /// A string or char literal: `&'a str`.
    Lit(Vec<u8>, Option<&'p syn::Ident>),
    /// `"…"?`: `Option<&'a str>`.
    OptLit(Vec<u8>, Option<&'p syn::Ident>),
    /// The built-in `digit` or `hex_digit`: `char`.
    Digit(DigitClass, Option<&'p syn::Ident>),
    /// `digit?`, `hex_digit?`: `Option<char>`.
    OptDigit(DigitClass, Option<&'p syn::Ident>),
    /// `digit{min,max}`, `hex_digit{min,max}` with an upper bound: `&'a str`
    /// when bound.
    Digits(DigitClass, usize, usize, Option<&'p syn::Ident>),
    /// `dec<T>(digit{min,max})` where `max` digits always fit in `T`: `T`.
    Dec(&'p syn::Type, usize, usize, Option<&'p syn::Ident>),
    /// `=>` between two elements: it matches nothing, and the elements as
    /// written - the fallback - carry it.
    Cut,
    /// The call of a rule whose whole body is one run (ADR 24 §8d): its
    /// elements, the rule, and the call's binding, the value of the rule's
    /// action on what they bound.
    Call(Vec<Fixed<'p>>, &'p Rule, Option<&'p syn::Ident>),
}

impl Fixed<'_> {
    fn binding(&self) -> Option<&syn::Ident> {
        match self {
            Fixed::Lit(_, b)
            | Fixed::OptLit(_, b)
            | Fixed::Digit(_, b)
            | Fixed::OptDigit(_, b)
            | Fixed::Digits(_, _, _, b)
            | Fixed::Dec(_, _, _, b)
            | Fixed::Call(_, _, b) => *b,
            Fixed::Cut => None,
        }
    }
}

/// Whether an action can run where a caller's run stands instead of in its
/// rule (ADR 24 §8d): an expression that cannot leave early - no `return`,
/// no `?`, no macro, which might hide either - and that names nothing the
/// rule's own function has and the caller's has differently: the input, a
/// span.
fn action_inlines(action: &TokenStream, input: &syn::Ident) -> bool {
    let tokens: Vec<TokenTree> = action.clone().into_iter().collect();
    tokens.iter().enumerate().all(|(k, t)| match t {
        TokenTree::Ident(i) => {
            let is_macro = matches!(tokens.get(k + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                && matches!(tokens.get(k + 2), Some(TokenTree::Group(_)));
            !(is_macro || i == "return" || i == "_span" || i == "self" || i == input)
        }
        TokenTree::Punct(p) => p.as_char() != '?',
        TokenTree::Group(g) => action_inlines(&g.stream(), input),
        TokenTree::Literal(_) => true,
    })
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

    /// Is `p` the built-in `digit` or `hex_digit` (not a rule of the
    /// grammar's own by that name), which, and with what binding?
    fn builtin_digit<'p>(
        &self,
        p: &'p ModelPattern,
    ) -> Option<(DigitClass, Option<&'p syn::Ident>)> {
        match p {
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                binding,
                ..
            } if generics.is_empty() && args.is_empty() => {
                let class = [DigitClass::Dec, DigitClass::Hex]
                    .into_iter()
                    .find(|c| rule_path.is_ident(c.builtin()))?;
                (!self.user_rules.contains(class.builtin())).then_some((class, binding.as_ref()))
            }
            _ => None,
        }
    }

    /// What `p` is as an element of a run, if it is one. `outer` is false for
    /// the elements of a called rule (§8d): those take in no call of their
    /// own, and no `frame_end`, which stands for the caller's boundary here.
    fn fixed_element<'p>(&'p self, p: &'p ModelPattern, outer: bool) -> Option<Fixed<'p>> {
        match p {
            ModelPattern::Cut(_) => Some(Fixed::Cut),
            // The frame boundary, a literal the frame check resolved for this
            // rule (§8a).
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                binding,
            } if outer
                && generics.is_empty()
                && args.is_empty()
                && rule_path.is_ident(winnow_grammar_model::frame::FRAME_END)
                && !self
                    .user_rules
                    .contains(winnow_grammar_model::frame::FRAME_END) =>
            {
                let b = self.current_boundary.borrow().clone()?;
                (!b.is_empty()).then(|| Fixed::Lit(b.into_bytes(), binding.as_ref()))
            }
            ModelPattern::RuleCall {
                rule_path,
                generics,
                args,
                binding,
            } if outer && generics.is_empty() && args.is_empty() => {
                let rule = rule_path
                    .get_ident()
                    .and_then(|name| self.grammar.rules.iter().find(|r| r.name == *name));
                rule.and_then(|rule| {
                    self.inlined_rule(rule)
                        .map(|elements| Fixed::Call(elements, rule, binding.as_ref()))
                })
                .or_else(|| self.builtin_digit(p).map(|(c, b)| Fixed::Digit(c, b)))
            }
            ModelPattern::Lit { lit, binding, .. } => {
                let bytes = Self::literal_bytes(lit)?;
                (!bytes.is_empty()).then_some(Fixed::Lit(bytes, binding.as_ref()))
            }
            ModelPattern::Optional(inner, _) => match &**inner {
                ModelPattern::Lit { lit, binding, .. } => {
                    let bytes = Self::literal_bytes(lit)?;
                    (!bytes.is_empty()).then_some(Fixed::OptLit(bytes, binding.as_ref()))
                }
                other => self
                    .builtin_digit(other)
                    .map(|(c, b)| Fixed::OptDigit(c, b)),
            },
            ModelPattern::Bounded {
                pattern,
                min,
                max: Some(max),
                ..
            } if *max <= MAX_DIGITS && min <= max && *max > 0 => self
                .builtin_digit(pattern)
                .map(|(c, b)| Fixed::Digits(c, *min, *max, b)),
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
                // `dec` reads base 10: a run of hex digits is left to it.
                let (DigitClass::Dec, _) = self.builtin_digit(pattern)? else {
                    return None;
                };
                (min <= max && *max > 0 && *max <= digits_that_fit(ty)?).then_some(Fixed::Dec(
                    ty,
                    (*min).max(1),
                    *max,
                    binding.as_ref(),
                ))
            }
            other => self.builtin_digit(other).map(|(c, b)| Fixed::Digit(c, b)),
        }
    }

    /// The elements of `rule` if a caller's run may take them in place of a
    /// call (ADR 24 §8d): a lexical rule of one alternative, no parameters,
    /// no span, whose body is a run of the elements above and nothing else,
    /// and whose action can run where the call stood.
    fn inlined_rule<'p>(&'p self, rule: &'p Rule) -> Option<Vec<Fixed<'p>>> {
        let [v] = &rule.variants[..] else { return None };
        if !rule.is_lexical
            || !rule.params.is_empty()
            || !rule.generics.params.is_empty()
            || rule.generics.where_clause.is_some()
            || v.with_span
            || !action_inlines(&v.action, &self.input_ident)
        {
            return None;
        }
        let elements = v
            .pattern
            .iter()
            .map(|p| self.fixed_element(p, false))
            .collect::<Option<Vec<_>>>()?;
        elements
            .iter()
            .any(|f| !matches!(f, Fixed::Cut))
            .then_some(elements)
    }

    /// The runs among `shapes`: maximal stretches of elements, without a
    /// `=>` at either end, that hold at least two elements other than `=>`.
    fn runs(shapes: &[Option<Fixed>]) -> Vec<(usize, usize)> {
        let is_cut = |k: usize| matches!(shapes[k], Some(Fixed::Cut));
        let mut runs = Vec::new();
        let mut i = 0;
        while i < shapes.len() {
            if shapes[i].is_none() {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < shapes.len() && shapes[j].is_some() {
                j += 1;
            }
            let next = j;
            let (mut a, mut z) = (i, j);
            while a < z && is_cut(a) {
                a += 1;
            }
            while z > a && is_cut(z - 1) {
                z -= 1;
            }
            if (a..z).filter(|&k| !is_cut(k)).count() >= 2 {
                runs.push((a, z));
            }
            i = next;
        }
        runs
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
        let shapes: Vec<Option<Fixed>> = patterns
            .iter()
            .map(|p| self.fixed_element(p, true))
            .collect();
        let runs = Self::runs(&shapes);
        if runs.is_empty() {
            return None;
        }
        let mut steps = TokenStream::new();
        let mut in_cut = in_cut;
        let mut runs = runs.into_iter().peekable();
        let mut i = 0;
        while i < patterns.len() {
            if let Some(&(a, z)) = runs.peek() {
                if a == i {
                    runs.next();
                    let run: Vec<&Fixed> =
                        shapes[a..z].iter().map(|s| s.as_ref().unwrap()).collect();
                    steps.extend(self.fixed_run(&run, &patterns[a..z], in_cut));
                    in_cut |= run.iter().any(|f| matches!(f, Fixed::Cut));
                    i = z;
                    continue;
                }
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

    /// The indexed match of one element `f`, its value in `v`. What the
    /// match yields is pushed to `yields`; what has to run once the whole
    /// run matched - a called rule's action (§8d) - goes to `after`.
    fn fixed_match(
        &self,
        f: &Fixed,
        v: &syn::Ident,
        fail: &TokenStream,
        yields: &mut Vec<syn::Ident>,
        after: &mut TokenStream,
    ) -> TokenStream {
        let span = Span::mixed_site();
        let s = format_ident!("__fixed_s", span = span);
        let b = format_ident!("__fixed_b", span = span);
        let at = format_ident!("__fixed_at", span = span);
        if f.binding().is_some() && !matches!(f, Fixed::Call(..)) {
            yields.push(v.clone());
        }
        // An element nobody named is matched and not kept.
        let v_pat = if f.binding().is_some() {
            quote_spanned! {span=> #v }
        } else {
            quote_spanned! {span=> _ }
        };
        match f {
            Fixed::Cut => TokenStream::new(),
            // The called rule's elements stand in the call's place; its
            // action runs on what they bound once the whole run matched, so
            // that a mismatch after it leaves nothing done twice when the
            // elements run as written.
            Fixed::Call(elements, rule, _) => {
                let mut matches = TokenStream::new();
                let mut lets = TokenStream::new();
                for (k, inner) in elements.iter().enumerate() {
                    let w = format_ident!("{}_{}", v, k, span = span);
                    matches.extend(self.fixed_match(inner, &w, fail, yields, after));
                    if let Some(name) = inner.binding() {
                        lets.extend(quote_spanned! {span=>
                            #[allow(unused_variables)]
                            let #name = #w;
                        });
                    }
                }
                let ret = &rule.return_type;
                let action = &rule.variants[0].action;
                let input = &self.input_ident;
                // The rule's action sees what it sees in its own function:
                // its bindings and the context (`variants.rs`).
                let state = quote! { let _state = &mut #input.state; };
                after.extend(quote_spanned! {span=>
                    let #v_pat: #ret = {
                        #lets
                        #state
                        { #action }
                    };
                });
                matches
            }
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
            Fixed::Digit(class, _) => {
                let test = class.test();
                quote_spanned! {span=>
                    let #v_pat = match #b.get(#at) {
                        ::core::option::Option::Some(c) if #test(c) => { #at += 1; *c as char }
                        _ => break 'fixed #fail,
                    };
                }
            }
            Fixed::OptDigit(class, _) => {
                let test = class.test();
                quote_spanned! {span=>
                    let #v_pat = match #b.get(#at) {
                        ::core::option::Option::Some(c) if #test(c) => { #at += 1; ::core::option::Option::Some(*c as char) }
                        _ => ::core::option::Option::None,
                    };
                }
            }
            Fixed::Digits(class, min, max, b_) => {
                // A run of digits is text in both kinds of grammar; over
                // bytes the class makes it so (ADR 25 D2).
                let test = class.test();
                let ascii = class.ascii_class();
                let value = b_.map(|_| {
                    if self.is_bytes() {
                        quote_spanned! {span=> let #v = ::winnow_grammar::rt::bytes::class_text(#ascii, &#s[start..#at]); }
                    } else {
                        quote_spanned! {span=> let #v = &#s[start..#at]; }
                    }
                });
                quote_spanned! {span=>
                    let start = #at;
                    while #at - start < #max && #b.get(#at).is_some_and(#test) {
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
        }
    }

    /// Does the run yield anything from its indexed match - a bound element,
    /// or one a called rule's action reads?
    fn yields_values(f: &Fixed) -> bool {
        match f {
            Fixed::Call(elements, _, _) => elements.iter().any(Self::yields_values),
            other => other.binding().is_some(),
        }
    }

    fn fixed_run(&self, run: &[&Fixed], patterns: &[ModelPattern], in_cut: bool) -> TokenStream {
        let rts = self.rts();
        let span = Span::mixed_site();
        let input = &self.input_ident;
        let s = format_ident!("__fixed_s", span = span);
        let b = format_ident!("__fixed_b", span = span);
        let at = format_ident!("__fixed_at", span = span);

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
        // What a failed match breaks out with: the run either yields its
        // values or, with none, only whether it matched.
        let fail = if run.iter().any(|f| Self::yields_values(f)) {
            quote_spanned! {span=> ::core::option::Option::None }
        } else {
            quote_spanned! {span=> false }
        };
        let mut matches = TokenStream::new();
        let mut yields = Vec::new();
        let mut after = TokenStream::new();
        // The run's bindings, and the value each is handed.
        let mut names = Vec::new();
        let mut values = Vec::new();
        for (k, f) in run.iter().enumerate() {
            let v = format_ident!("__fixed_{}", k, span = span);
            matches.extend(self.fixed_match(f, &v, &fail, &mut yields, &mut after));
            if let Some(name) = f.binding() {
                names.push(name.clone());
                values.push(v);
            }
        }
        // The fallback: the same elements, generated as they always were,
        // a `=>` among them included, their bindings handed out as the
        // indexed match's would be.
        let mut cut = in_cut;
        let fallback: Vec<TokenStream> = patterns
            .iter()
            .map(|p| {
                if let ModelPattern::Cut(_) = p {
                    cut = true;
                }
                self.generate_step(p, cut, true)
            })
            .collect();
        let prelude = quote_spanned! {span=>
            #slice_of_rest
            let mut #at = 0usize;
        };
        // The diagnosing pass keeps the elements: an optional one that did
        // not match leaves "also possible here" behind, which the indexed
        // match has no reason to know about.
        let fast_pass =
            quote_spanned! {span=> !<__WgEr as ::winnow_grammar::Diagnostics>::RECORDING };
        if yields.is_empty() {
            // Nothing yielded: no tuple, so no `()` for Clippy to point at.
            return quote_spanned! {span=>
                {
                    #prelude
                    let found = #fast_pass && 'fixed: {
                        #matches
                        true
                    };
                    if found {
                        #rts::advance(#input, #at);
                        #after
                    } else {
                        #(#fallback)*
                    }
                }
            };
        }
        let found = quote_spanned! {span=>
            #prelude
            let found = if #fast_pass {
                'fixed: {
                    #matches
                    ::core::option::Option::Some((#(#yields,)*))
                }
            } else {
                ::core::option::Option::None
            };
        };
        if names.is_empty() {
            return quote_spanned! {span=>
                {
                    #found
                    match found {
                        ::core::option::Option::Some((#(#yields,)*)) => {
                            #rts::advance(#input, #at);
                            #after
                        }
                        ::core::option::Option::None => {
                            #(#fallback)*
                        }
                    }
                }
            };
        }
        quote_spanned! {span=>
            let (#(#names,)*) = {
                #found
                match found {
                    ::core::option::Option::Some((#(#yields,)*)) => {
                        #rts::advance(#input, #at);
                        #after
                        (#(#values,)*)
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

#[cfg(test)]
mod tests {
    use super::Codegen;
    use quote::quote;
    use winnow_grammar_model::parse_grammar;

    /// Whether a run that calls `callee` takes its elements in (ADR 24 §8d).
    fn inlined(rules: proc_macro2::TokenStream, callee: &str) -> bool {
        let parsed = parse_grammar::<crate::WinnowBackend>(quote! {
            grammar G { #rules }
        })
        .unwrap();
        let cg = Codegen::new(&parsed.grammar, parsed.frames, parsed.streams);
        let rule = cg.grammar.rules.iter().find(|r| r.name == callee).unwrap();
        cg.inlined_rule(rule).is_some()
    }

    #[test]
    fn a_rule_of_one_run_is_taken_in() {
        assert!(inlined(
            quote! {
                pub T -> i32 = neg:"-"? w:dec<i32>(digit{1,2}) "." f:digit -> {
                    let _ = _state;
                    if neg.is_some() { -w } else { w + (f as i32) }
                }
            },
            "T"
        ));
        assert!(inlined(quote! { S -> () = ";" => "x" -> { } }, "S"));
    }

    #[test]
    fn hex_digit_is_a_fixed_element_unless_shadowed() {
        assert!(inlined(
            quote! { T -> (char, Option<char>, &'a str) = a:hex_digit b:hex_digit? c:hex_digit{2} -> { (a, b, c) } },
            "T"
        ));
        assert!(!inlined(
            quote! {
                hex_digit -> char = "z" -> { 'z' }
                T -> (char, char) = a:hex_digit b:hex_digit -> { (a, b) }
            },
            "T"
        ));
        // `dec` reads base 10; over hex digits it keeps `rt::dec`.
        assert!(!inlined(
            quote! { T -> u8 = "x" v:dec<u8>(hex_digit{2}) -> { v } },
            "T"
        ));
    }

    #[test]
    fn an_alternative_a_call_or_an_element_of_no_fixed_shape_is_called() {
        assert!(!inlined(
            quote! { T -> char = d:digit -> { d } | "x" -> { 'x' } },
            "T"
        ));
        assert!(!inlined(
            quote! {
                D -> char = d:digit -> { d }
                T -> (char, char) = a:D b:digit -> { (a, b) }
            },
            "T"
        ));
        assert!(!inlined(quote! { T -> &'a str = d:digit+ -> { d } }, "T"));
        assert!(!inlined(quote! { T -> () = "" "x" -> { } }, "T"));
    }

    #[test]
    fn an_action_that_may_leave_early_or_sees_the_rule_s_own_input_is_called() {
        assert!(!inlined(
            quote! { T -> char = d:digit -> { return Ok(d) } },
            "T"
        ));
        assert!(!inlined(
            quote! { T -> u32 = d:digit -> { d.to_digit(10)? } },
            "T"
        ));
        assert!(!inlined(
            quote! { T -> String = d:digit -> { format!("{d}") } },
            "T"
        ));
        assert!(!inlined(
            quote! { T -> usize = digit -> { input.state.rules.len() } },
            "T"
        ));
        assert!(!inlined(
            quote! { T -> (char, usize) = d:digit -> { (d, _span.start) } },
            "T"
        ));
    }
}
