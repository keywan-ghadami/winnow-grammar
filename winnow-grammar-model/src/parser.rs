// Moved from macros/src/parser.rs
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::parse::{Parse, ParseStream};
use syn::{token, Attribute, Generics, Ident, ItemUse, Lit, Path, Result, Token, Type};

mod rt {
    use syn::ext::IdentExt;
    use syn::parse::discouraged::Speculative;
    use syn::parse::ParseStream;
    use syn::Result;

    pub fn attempt<T>(
        input: ParseStream,
        parser: impl FnOnce(ParseStream) -> Result<T>,
    ) -> Result<Option<T>> {
        let fork = input.fork();
        match parser(&fork) {
            Ok(res) => {
                input.advance_to(&fork);
                Ok(Some(res))
            }
            Err(_) => Ok(None),
        }
    }

    pub fn parse_ident(input: ParseStream) -> Result<syn::Ident> {
        input.call(syn::Ident::parse_any)
    }
}

pub mod kw {
    syn::custom_keyword!(grammar);
    syn::custom_keyword!(rule);
    syn::custom_keyword!(paren);
    syn::custom_keyword!(brace);
    syn::custom_keyword!(recover);
    syn::custom_keyword!(peek);
    syn::custom_keyword!(not);
    syn::custom_keyword!(until);
    syn::custom_keyword!(import);
    syn::custom_keyword!(fail);
    syn::custom_keyword!(count);
    syn::custom_keyword!(fold);
    syn::custom_keyword!(par_fold);
    syn::custom_keyword!(lex);
    syn::custom_keyword!(spaced);
}

fn parse_path_no_args(input: ParseStream) -> Result<Path> {
    let leading_colon = if input.peek(Token![::]) {
        Some(input.parse::<Token![::]>()?)
    } else {
        None
    };

    let mut segments = syn::punctuated::Punctuated::new();
    loop {
        let ident: Ident = rt::parse_ident(input)?;
        let arguments = syn::PathArguments::None;
        segments.push_value(syn::PathSegment { ident, arguments });

        if input.peek(Token![::]) {
            let punct = input.parse::<Token![::]>()?;
            segments.push_punct(punct);
        } else {
            break;
        }
    }

    Ok(Path {
        leading_colon,
        segments,
    })
}

#[derive(Debug, Clone)]
pub struct ExternRule {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub generics: Generics,
    pub params: Vec<RuleParameter>,
    pub return_type: Type,
}

impl Parse for ExternRule {
    fn parse(input: ParseStream) -> Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let _ = input.parse::<Token![extern]>()?;
        let _ = input.parse::<kw::rule>()?;
        let name = rt::parse_ident(input)?;
        let generics: Generics = input.parse()?;

        let params = if input.peek(token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            let mut params = Vec::new();
            while !content.is_empty() {
                params.push(content.parse()?);
                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
            params
        } else {
            Vec::new()
        };

        let _ = input.parse::<Token![->]>()?;
        let return_type = input.parse::<Type>()?;
        let _ = input.parse::<Token![;]>()?;

        Ok(ExternRule {
            attrs,
            name,
            generics,
            params,
            return_type,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ImportedGrammar {
    pub path: Path,
    pub alias: Ident,
}

impl Parse for ImportedGrammar {
    fn parse(input: ParseStream) -> Result<Self> {
        let _ = input.parse::<kw::import>()?;
        let path = input.parse::<Path>()?; // No `grammar` keyword in path parsing
        let _ = input.parse::<Token![as]>()?;
        let alias = input.parse::<Ident>()?;
        let _ = input.parse::<Token![;]>()?;
        Ok(ImportedGrammar { path, alias })
    }
}

#[derive(Debug, Clone)]
pub struct GrammarDefinition {
    pub name: Ident,
    pub inherits: Option<InheritanceSpec>,
    pub uses: Vec<ItemUse>,
    pub rules: Vec<Rule>,
    pub extern_rules: Vec<ExternRule>,
    pub imports: Vec<ImportedGrammar>,
}

impl Parse for GrammarDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse top-level imports that might appear before `grammar Name { ... }`
        let mut top_level_imports = Vec::new();
        while input.peek(kw::import) {
            top_level_imports.push(input.parse()?);
        }

        let _ = input.parse::<kw::grammar>()?;
        let name = rt::parse_ident(input)?;

        let inherits = if input.peek(Token![:]) {
            Some(input.parse::<InheritanceSpec>()?)
        } else {
            None
        };

        let content;
        let _ = syn::braced!(content in input);

        let mut uses = Vec::new();
        let mut rules = Vec::new();
        let mut extern_rules = Vec::new();
        let mut nested_imports = Vec::new();

        while !content.is_empty() {
            if content.peek(Token![use]) {
                uses.push(content.parse()?);
            } else if content.peek(kw::import) {
                nested_imports.push(content.parse()?);
            } else if content.peek(Token![extern]) {
                extern_rules.push(content.parse()?);
            } else {
                // Try parsing as rule (it might have attributes)
                rules.push(content.parse()?);
            }
        }

        let mut imports = top_level_imports;
        imports.extend(nested_imports);

        Ok(GrammarDefinition {
            name,
            inherits,
            uses,
            rules,
            extern_rules,
            imports,
        })
    }
}

impl ToTokens for GrammarDefinition {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = &self.name;
        let inherits = &self.inherits;
        let uses = &self.uses;
        let rules = &self.rules;
        // extern_rules and imports are not currently emitted in ToTokens as they are structural metadata

        tokens.append_all(quote! {
            grammar #name #inherits {
                #(#uses)*
                #(#rules)*
            }
        });
    }
}

#[derive(Debug, Clone)]
pub struct InheritanceSpec {
    pub name: Ident,
}

impl Parse for InheritanceSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        let _ = input.parse::<Token![:]>()?;
        let name = rt::parse_ident(input)?;
        Ok(InheritanceSpec { name })
    }
}

impl ToTokens for InheritanceSpec {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = &self.name;
        tokens.append_all(quote! { : #name });
    }
}

#[derive(Debug, Clone)]
pub struct RuleParameter {
    pub name: Ident,
    pub ty: Option<Type>,
}

impl Parse for RuleParameter {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        let ty = if input.peek(Token![:]) {
            let _ = input.parse::<Token![:]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        Ok(RuleParameter { name, ty })
    }
}

impl ToTokens for RuleParameter {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = &self.name;
        if let Some(ty) = &self.ty {
            tokens.append_all(quote! { #name : #ty });
        } else {
            tokens.append_all(quote! { #name });
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub attrs: Vec<Attribute>,
    pub is_pub: Option<Token![pub]>,
    pub name: Ident,
    pub generics: Generics,
    pub params: Vec<RuleParameter>,
    pub return_type: Type,
    pub variants: Vec<RuleVariant>,
}

impl Parse for Rule {
    fn parse(input: ParseStream) -> Result<Self> {
        let attrs = Attribute::parse_outer(input)?;

        let is_pub = if input.peek(Token![pub]) {
            Some(input.parse()?)
        } else {
            None
        };

        if input.peek(kw::rule) {
            let _ = input.parse::<kw::rule>()?;
        }
        let name = rt::parse_ident(input)?;

        let generics: Generics = input.parse()?;

        let params = if input.peek(token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            let mut params = Vec::new();
            while !content.is_empty() {
                params.push(content.parse()?);
                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
            params
        } else {
            Vec::new()
        };

        let return_type = if input.peek(Token![->]) {
            let _ = input.parse::<Token![->]>()?;
            input.parse::<Type>()?
        } else {
            syn::parse_quote!(())
        };

        let capture_span = if input.peek(Token![@]) && input.peek2(Token![=]) {
            let _ = input.parse::<Token![@]>()?;
            let _ = input.parse::<Token![=]>()?;
            true
        } else {
            let _ = input.parse::<Token![=]>()?;
            false
        };

        let variants = RuleVariant::parse_list(input, capture_span)?;

        Ok(Rule {
            attrs,
            is_pub,
            name,
            generics,
            params,
            return_type,
            variants,
        })
    }
}

impl ToTokens for Rule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let attrs = &self.attrs;
        let vis = &self.is_pub;
        let name = &self.name;
        let generics = &self.generics;
        let ret = &self.return_type;
        let variants = &self.variants;

        let params_tokens = if self.params.is_empty() {
            quote! {}
        } else {
            let params = &self.params;
            quote! { ( #(#params),* ) }
        };

        let mut variants_tokens = TokenStream::new();
        for (i, v) in variants.iter().enumerate() {
            if i > 0 {
                token::Or::default().to_tokens(&mut variants_tokens);
            }
            v.to_tokens(&mut variants_tokens);
        }

        // We don't have explicit access to capture_span here to re-emit it,
        // but RuleVariant knows about with_span which is derived from it.
        // However, standard ToTokens for Rule usually reconstructs the syntax.
        // If we want to support round-tripping or accurate ToTokens, we should store capture_span in Rule.
        // But for now, just emitting = is standard. If the variants use it, fine.

        tokens.append_all(quote! {
            #(#attrs)*
            #vis rule #name #generics #params_tokens -> #ret = #variants_tokens
        });
    }
}

impl Rule {
    pub fn parse_all(input: ParseStream) -> Result<Vec<Self>> {
        let mut rules = Vec::new();
        while !input.is_empty() {
            rules.push(input.parse()?);
        }
        Ok(rules)
    }
}

#[derive(Debug, Clone)]
pub struct RuleVariant {
    pub pattern: Vec<Pattern>,
    pub label: Option<String>,
    pub action: TokenStream,
    pub with_span: bool,
    pub is_explicit: bool,
}

impl RuleVariant {
    pub fn parse_list(input: ParseStream, capture_span: bool) -> Result<Vec<Self>> {
        let mut variants = Vec::new();
        loop {
            let mut pattern: Vec<Pattern> = Vec::new();
            while !input.is_empty()
                && !input.peek(Token![->])
                && !input.peek(Token![|])
                && !input.peek(Token![#])
                && !input.peek(kw::rule)
            {
                // Lookahead to detect start of next rule:
                // 1. Ident followed by `=` (e.g. `next_rule = ...`)
                if input.peek(Ident) && input.peek2(Token![=]) {
                    break;
                }
                // 2. Ident followed by `@` then `=`
                if input.peek(Ident) && input.peek2(Token![@]) && input.peek3(Token![=]) {
                    break;
                }
                // 3. `pub` keyword (e.g. `pub rule ...` or `pub next_rule ...`)
                if input.peek(Token![pub]) {
                    break;
                }

                pattern.push(input.parse()?);
            }

            let label = if input.peek(Token![#]) {
                let _ = input.parse::<Token![#]>()?;
                let lit: syn::LitStr = input.parse()?;
                Some(lit.value())
            } else {
                None
            };

            let mut is_explicit = false;
            let action = if input.peek(Token![->]) {
                is_explicit = true;
                let _ = input.parse::<Token![->]>()?;
                let content;
                syn::braced!(content in input);
                content.parse()?
            } else {
                let mut bindings = Vec::new();
                for p in &pattern {
                    p.collect_bindings(&mut bindings);
                }

                if bindings.is_empty() {
                    quote! { () }
                } else if bindings.len() == 1 {
                    let b = &bindings[0];
                    quote! { #b }
                } else {
                    quote! { ( #(#bindings),* ) }
                }
            };

            variants.push(RuleVariant {
                pattern,
                label,
                action,
                with_span: capture_span,
                is_explicit,
            });

            if input.peek(Token![|]) {
                let _ = input.parse::<Token![|]>()?;
            } else {
                break;
            }
        }
        Ok(variants)
    }
}

impl ToTokens for RuleVariant {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let pattern = &self.pattern;
        let action = &self.action;
        let label = if let Some(l) = &self.label {
            let l_lit = syn::LitStr::new(l, proc_macro2::Span::call_site());
            quote! { # #l_lit }
        } else {
            quote! {}
        };

        if self.is_explicit {
            tokens.append_all(quote! {
                #(#pattern)* #label -> { #action }
            });
        } else {
            tokens.append_all(quote! {
                #(#pattern)* #label
            });
        }
    }
}

#[derive(Debug, Clone)]
pub enum Argument {
    Positional(Pattern),
    Named(Ident, Pattern),
}

impl Parse for Argument {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(Ident) && input.peek2(Token![=]) {
            let name: Ident = input.parse()?;
            let _ = input.parse::<Token![=]>()?;
            let val: Pattern = input.parse()?;
            Ok(Argument::Named(name, val))
        } else {
            Ok(Argument::Positional(input.parse()?))
        }
    }
}

impl ToTokens for Argument {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Argument::Positional(p) => p.to_tokens(tokens),
            Argument::Named(n, p) => {
                n.to_tokens(tokens);
                token::Eq::default().to_tokens(tokens);
                p.to_tokens(tokens);
            }
        }
    }
}

/// A sequence of patterns with an optional action and label.
pub type GroupAlternative = (Vec<Pattern>, Option<TokenStream>, Option<String>);

#[derive(Debug, Clone)]
pub enum Pattern {
    Cut(Token![=>]),
    Lit {
        binding: Option<Ident>,
        lit: Lit,
    },
    RuleCall {
        binding: Option<Ident>,
        rule_path: Path,
        generics: Vec<Type>,
        args: Vec<Argument>,
    },
    Group {
        binding: Option<Ident>,
        alts: Vec<GroupAlternative>,
        token: token::Paren,
    },
    Bracketed {
        binding: Option<Ident>,
        patterns: Vec<Pattern>,
        token: token::Bracket,
    },
    Braced {
        binding: Option<Ident>,
        patterns: Vec<Pattern>,
        token: token::Brace,
    },
    Parenthesized {
        binding: Option<Ident>,
        patterns: Vec<Pattern>,
        kw_token: kw::paren,
        token: token::Paren,
    },
    Optional(Box<Pattern>, Token![?]),
    Repeat(Box<Pattern>, Token![*]),
    Plus(Box<Pattern>, Token![+]),
    /// `p{n}`, `p{n,}`, `p{n,m}` - a repetition with explicit bounds.
    Bounded {
        pattern: Box<Pattern>,
        min: usize,
        /// `None` for an open upper bound (`p{n,}`).
        max: Option<usize>,
        token: token::Brace,
    },
    SpanBinding(Box<Pattern>, Ident, Token![@]),
    Recover {
        binding: Option<Ident>,
        body: Box<Pattern>,
        sync: Box<Pattern>,
        kw_token: kw::recover,
    },
    Peek(Box<Pattern>, kw::peek),
    Not(Box<Pattern>, kw::not),
    Until {
        binding: Option<Ident>,
        pattern: Box<Pattern>,
        kw_token: kw::until,
    },
    Count {
        binding: Option<Ident>,
        pattern: Box<Pattern>,
        kw_token: kw::count,
    },
    Fold {
        binding: Option<Ident>,
        pattern: Box<Pattern>,
        // Boxed: a bare `syn::Expr` is ~260 bytes, and two of them would make
        // this variant four times the size of the next largest, a cost every
        // `Pattern` in the grammar would carry.
        init: Box<syn::Expr>,
        step: Box<syn::Expr>,
        /// `par_fold(rule, init, step, merge)`: how two accumulators combine.
        /// `None` for a plain `fold`.
        merge: Option<Box<syn::Expr>>,
        /// The span of the keyword, `fold` or `par_fold`.
        kw_span: proc_macro2::Span,
    },
    LexicalScope(Box<Pattern>, kw::lex),
    SpacedScope(Box<Pattern>, kw::spaced),
    Fail {
        message: Option<Lit>,
        kw_token: kw::fail,
    },
}

impl Pattern {
    fn wrap_sequence(patterns: Vec<Pattern>) -> Pattern {
        if patterns.len() == 1 {
            patterns.into_iter().next().unwrap()
        } else {
            Pattern::Group {
                binding: None,
                alts: vec![(patterns, None, None)],
                token: token::Paren::default(),
            }
        }
    }

    pub fn collect_bindings(&self, acc: &mut Vec<Ident>) {
        match self {
            Pattern::Lit { binding, .. } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                }
            }
            Pattern::RuleCall { binding, .. } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                }
            }
            Pattern::Group { binding, alts, .. } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    for (pats, action, _) in alts {
                        if action.is_none() {
                            for p in pats {
                                p.collect_bindings(acc);
                            }
                        }
                    }
                }
            }
            Pattern::Bracketed {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    for p in patterns {
                        p.collect_bindings(acc);
                    }
                }
            }
            Pattern::Braced {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    for p in patterns {
                        p.collect_bindings(acc);
                    }
                }
            }
            Pattern::Parenthesized {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    for p in patterns {
                        p.collect_bindings(acc);
                    }
                }
            }
            Pattern::Optional(p, _) => p.collect_bindings(acc),
            Pattern::Repeat(p, _) => p.collect_bindings(acc),
            Pattern::Plus(p, _) => p.collect_bindings(acc),
            Pattern::Bounded { pattern, .. } => pattern.collect_bindings(acc),
            Pattern::SpanBinding(p, id, _) => {
                acc.push(id.clone());
                p.collect_bindings(acc);
            }
            Pattern::Recover { binding, body, .. } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    body.collect_bindings(acc);
                }
            }
            Pattern::Peek(p, _) => p.collect_bindings(acc),
            Pattern::Not(p, _) => p.collect_bindings(acc),
            Pattern::Until {
                binding, pattern, ..
            } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    pattern.collect_bindings(acc);
                }
            }
            Pattern::Count {
                binding, pattern, ..
            } => {
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    pattern.collect_bindings(acc);
                }
            }
            Pattern::Fold {
                binding, pattern, ..
            } => {
                // The accumulator is the value, so a binding names it; the
                // element's own bindings live inside `step` and are not visible
                // to the surrounding action.
                if let Some(b) = binding {
                    acc.push(b.clone());
                } else {
                    pattern.collect_bindings(acc);
                }
            }
            Pattern::LexicalScope(p, _) => p.collect_bindings(acc),
            Pattern::SpacedScope(p, _) => p.collect_bindings(acc),
            Pattern::Fail { .. } => {}
            Pattern::Cut(_) => {}
        }
    }

    pub fn has_binding(&self) -> bool {
        match self {
            Pattern::Lit { binding, .. } => binding.is_some(),
            Pattern::RuleCall { binding, .. } => binding.is_some(),
            Pattern::Group { binding, alts, .. } => {
                if binding.is_some() {
                    return true;
                }
                alts.iter().any(|(pats, action, _)| {
                    action.is_none() && pats.iter().any(|p| p.has_binding())
                })
            }
            Pattern::Bracketed {
                binding, patterns, ..
            } => binding.is_some() || patterns.iter().any(|p| p.has_binding()),
            Pattern::Braced {
                binding, patterns, ..
            } => binding.is_some() || patterns.iter().any(|p| p.has_binding()),
            Pattern::Parenthesized {
                binding, patterns, ..
            } => binding.is_some() || patterns.iter().any(|p| p.has_binding()),
            Pattern::Optional(p, _) => p.has_binding(),
            Pattern::Repeat(p, _) => p.has_binding(),
            Pattern::Plus(p, _) => p.has_binding(),
            Pattern::Bounded { pattern, .. } => pattern.has_binding(),
            Pattern::SpanBinding(..) => true,
            Pattern::Recover {
                binding,
                body,
                sync,
                ..
            } => binding.is_some() || body.has_binding() || sync.has_binding(),
            Pattern::Peek(p, _) => p.has_binding(),
            Pattern::Not(p, _) => p.has_binding(),
            Pattern::Until {
                binding, pattern, ..
            } => binding.is_some() || pattern.has_binding(),
            Pattern::Count {
                binding, pattern, ..
            } => binding.is_some() || pattern.has_binding(),
            Pattern::Fold {
                binding, pattern, ..
            } => binding.is_some() || pattern.has_binding(),
            Pattern::LexicalScope(p, _) => p.has_binding(),
            Pattern::SpacedScope(p, _) => p.has_binding(),
            Pattern::Fail { .. } => false,
            Pattern::Cut(_) => false,
        }
    }
}

/// Is the brace group at the cursor a repetition bound (`{2}`, `{1,}`, `{1,2}`)
/// rather than the braced-delimiter pattern (`{ inner }`)? Decided by the first
/// token inside: a bound always starts with an integer, and a delimiter pattern
/// matching a bare integer literal (`{ 2 }`) has no use - the grammar would be
/// matching the character `2` between braces, which is written `"{" "2" "}"`.
fn starts_repeat_bounds(input: ParseStream) -> bool {
    if !input.peek(token::Brace) {
        return false;
    }
    let fork = input.fork();
    let peek_inside = |s: ParseStream| -> Result<bool> {
        let content;
        let _ = syn::braced!(content in s);
        Ok(content.peek(syn::LitInt))
    };
    peek_inside(&fork).unwrap_or(false)
}

/// Would this brace content be read back as a repetition bound rather than as
/// a braced-delimiter pattern? The mirror of [`starts_repeat_bounds`], for the
/// re-emission side.
fn starts_with_int_literal(patterns: &[Pattern]) -> bool {
    matches!(
        patterns.first(),
        Some(Pattern::Lit {
            lit: Lit::Int(_),
            ..
        })
    )
}

/// Parses `{n}`, `{n,}` or `{n,m}`. Only called once [`starts_repeat_bounds`]
/// has established that this brace group is a bound, so every error here is a
/// malformed bound and is reported as such.
fn parse_repeat_bounds(input: ParseStream) -> Result<(usize, Option<usize>, token::Brace)> {
    let content;
    let token = syn::braced!(content in input);

    let min_lit: syn::LitInt = content.parse()?;
    let min: usize = min_lit.base10_parse()?;

    let max = if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
        if content.is_empty() {
            None
        } else {
            let max_lit: syn::LitInt = content.parse()?;
            let max: usize = max_lit.base10_parse()?;
            if max == 0 {
                return Err(syn::Error::new(
                    max_lit.span(),
                    "an upper bound of 0 matches nothing - remove the pattern instead",
                ));
            }
            if max < min {
                return Err(syn::Error::new(
                    max_lit.span(),
                    format!("upper bound {max} is below the lower bound {min}"),
                ));
            }
            Some(max)
        }
    } else {
        if min == 0 {
            return Err(syn::Error::new(
                min_lit.span(),
                "`{0}` matches nothing - remove the pattern instead",
            ));
        }
        Some(min)
    };

    if !content.is_empty() {
        return Err(
            content.error("expected the end of the repetition bound: `{n}`, `{n,}` or `{n,m}`")
        );
    }

    Ok((min, max, token))
}

impl Parse for Pattern {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut pat = parse_atom(input)?;

        loop {
            if input.peek(Token![*]) {
                let token = input.parse::<Token![*]>()?;
                pat = Pattern::Repeat(Box::new(pat), token);
            } else if input.peek(Token![+]) {
                let token = input.parse::<Token![+]>()?;
                pat = Pattern::Plus(Box::new(pat), token);
            } else if input.peek(Token![?]) {
                let token = input.parse::<Token![?]>()?;
                pat = Pattern::Optional(Box::new(pat), token);
            } else if starts_repeat_bounds(input) {
                // `p{n,m}`. A brace group is also the *braced delimiter*
                // pattern (`{ inner }`), so only one whose content starts with
                // an integer is read as a bound - and once it does, a malformed
                // bound is an error rather than a confusing "expected pattern"
                // from the delimiter parser.
                let (min, max, token) = parse_repeat_bounds(input)?;
                pat = Pattern::Bounded {
                    pattern: Box::new(pat),
                    min,
                    max,
                    token,
                };
            } else if input.peek(Token![@]) {
                let token = input.parse::<Token![@]>()?;
                let ident = input.parse::<Ident>()?;
                pat = Pattern::SpanBinding(Box::new(pat), ident, token);
            } else {
                break;
            }
        }
        Ok(pat)
    }
}

impl ToTokens for Pattern {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Pattern::Cut(_) => {
                token::FatArrow::default().to_tokens(tokens);
            }
            Pattern::Lit { binding, lit } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                lit.to_tokens(tokens);
            }
            Pattern::RuleCall {
                binding,
                rule_path,
                generics,
                args,
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                rule_path.to_tokens(tokens);
                if !generics.is_empty() {
                    token::Lt::default().to_tokens(tokens);
                    for (i, t) in generics.iter().enumerate() {
                        if i > 0 {
                            token::Comma::default().to_tokens(tokens);
                        }
                        t.to_tokens(tokens);
                    }
                    token::Gt::default().to_tokens(tokens);
                }
                if !args.is_empty() {
                    token::Paren::default().surround(tokens, |t| {
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 {
                                token::Comma::default().to_tokens(t);
                            }
                            a.to_tokens(t);
                        }
                    });
                }
            }
            Pattern::Group { binding, alts, .. } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                token::Paren::default().surround(tokens, |t| {
                    for (i, (seq, action, label)) in alts.iter().enumerate() {
                        if i > 0 {
                            token::Or::default().to_tokens(t);
                        }
                        for p in seq {
                            p.to_tokens(t);
                        }
                        if let Some(a) = action {
                            token::RArrow::default().to_tokens(t);
                            token::Brace::default().surround(t, |t2| a.to_tokens(t2));
                        }
                        if let Some(l) = label {
                            token::Pound::default().to_tokens(t);
                            syn::LitStr::new(l, proc_macro2::Span::call_site()).to_tokens(t);
                        }
                    }
                });
            }
            Pattern::Bracketed {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                token::Bracket::default().surround(tokens, |t| {
                    for p in patterns {
                        p.to_tokens(t);
                    }
                });
            }
            Pattern::Braced {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                // Re-emitting `{ 2 … }` would come back as a repetition bound
                // on the preceding pattern, so the one content that needs the
                // keyword form goes out in it.
                if starts_with_int_literal(patterns) {
                    kw::brace::default().to_tokens(tokens);
                    token::Paren::default().surround(tokens, |t| {
                        for p in patterns {
                            p.to_tokens(t);
                        }
                    });
                } else {
                    token::Brace::default().surround(tokens, |t| {
                        for p in patterns {
                            p.to_tokens(t);
                        }
                    });
                }
            }
            Pattern::Parenthesized {
                binding, patterns, ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                kw::paren::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    for p in patterns {
                        p.to_tokens(t);
                    }
                });
            }
            Pattern::Optional(p, _) => {
                p.to_tokens(tokens);
                token::Question::default().to_tokens(tokens);
            }
            Pattern::Repeat(p, _) => {
                p.to_tokens(tokens);
                token::Star::default().to_tokens(tokens);
            }
            Pattern::Plus(p, _) => {
                p.to_tokens(tokens);
                token::Plus::default().to_tokens(tokens);
            }
            Pattern::Bounded {
                pattern, min, max, ..
            } => {
                pattern.to_tokens(tokens);
                token::Brace::default().surround(tokens, |t| {
                    proc_macro2::Literal::usize_unsuffixed(*min).to_tokens(t);
                    match max {
                        Some(m) if m == min => {}
                        Some(m) => {
                            token::Comma::default().to_tokens(t);
                            proc_macro2::Literal::usize_unsuffixed(*m).to_tokens(t);
                        }
                        None => token::Comma::default().to_tokens(t),
                    }
                });
            }
            Pattern::SpanBinding(p, id, _) => {
                p.to_tokens(tokens);
                token::At::default().to_tokens(tokens);
                id.to_tokens(tokens);
            }
            Pattern::Recover {
                binding,
                body,
                sync,
                ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                kw::recover::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    body.to_tokens(t);
                    token::Comma::default().to_tokens(t);
                    sync.to_tokens(t);
                });
            }
            Pattern::Peek(p, _) => {
                kw::peek::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    p.to_tokens(t);
                });
            }
            Pattern::Not(p, _) => {
                kw::not::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    p.to_tokens(t);
                });
            }
            Pattern::Until {
                binding, pattern, ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                kw::until::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    pattern.to_tokens(t);
                });
            }
            Pattern::Count {
                binding, pattern, ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                kw::count::default().to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    pattern.to_tokens(t);
                });
            }
            Pattern::Fold {
                binding,
                pattern,
                init,
                step,
                merge,
                ..
            } => {
                if let Some(b) = binding {
                    b.to_tokens(tokens);
                    token::Colon::default().to_tokens(tokens);
                }
                if merge.is_some() {
                    kw::par_fold::default().to_tokens(tokens);
                } else {
                    kw::fold::default().to_tokens(tokens);
                }
                token::Paren::default().surround(tokens, |t| {
                    pattern.to_tokens(t);
                    token::Comma::default().to_tokens(t);
                    init.to_tokens(t);
                    token::Comma::default().to_tokens(t);
                    step.to_tokens(t);
                    if let Some(m) = merge {
                        token::Comma::default().to_tokens(t);
                        m.to_tokens(t);
                    }
                });
            }
            Pattern::LexicalScope(pattern, kw_token) => {
                kw_token.to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    pattern.to_tokens(t);
                });
            }
            Pattern::SpacedScope(pattern, kw_token) => {
                kw_token.to_tokens(tokens);
                token::Paren::default().surround(tokens, |t| {
                    pattern.to_tokens(t);
                });
            }
            Pattern::Fail { message, kw_token } => {
                kw_token.to_tokens(tokens);
                if let Some(m) = message {
                    token::Paren::default().surround(tokens, |t| {
                        m.to_tokens(t);
                    });
                }
            }
        }
    }
}

fn parse_atom(input: ParseStream) -> Result<Pattern> {
    // 1. Check for binding
    let binding = rt::attempt(input, |input| {
        let id: Ident = input.parse()?;
        let _ = input.parse::<Token![:]>()?;
        Ok(id)
    })?;

    if input.peek(Token![=>]) {
        if binding.is_some() {
            return Err(input.error("Cut operator cannot be bound."));
        }
        let token = input.parse::<Token![=>]>()?;
        Ok(Pattern::Cut(token))
    } else if input.peek(Token![!]) {
        Err(input
            .error("The '!' operator is not supported. Use 'not(pattern)' for negative lookahead."))
    } else if input.peek(Token![&]) {
        Err(input.error(
            "The '&' operator is not supported. Use 'peek(pattern)' for positive lookahead.",
        ))
    } else if input.peek(Token![~]) {
        Err(input.error("The '~' operator is not supported. Use the '=>' cut operator instead"))
    } else if input.peek(Lit) {
        let lit: Lit = input.parse()?;
        // Char literals are preserved as is.
        Ok(Pattern::Lit { binding, lit })
    } else if input.peek(token::Bracket) {
        let content;
        let token = syn::bracketed!(content in input);
        Ok(Pattern::Bracketed {
            binding,
            patterns: parse_pattern_list(&content)?,
            token,
        })
    } else if input.peek(token::Brace) {
        let content;
        let token = syn::braced!(content in input);
        Ok(Pattern::Braced {
            binding,
            patterns: parse_pattern_list(&content)?,
            token,
        })
    } else if input.peek(kw::paren) {
        let kw = input.parse::<kw::paren>()?;
        let content;
        let token = syn::parenthesized!(content in input);
        Ok(Pattern::Parenthesized {
            binding,
            patterns: parse_pattern_list(&content)?,
            kw_token: kw,
            token,
        })
    } else if input.peek(kw::brace) {
        // The keyword form of `{ pattern }`, for the one content a brace
        // group can hold that the bare form cannot express: a leading integer
        // literal, which reads as a repetition bound. Same role `paren(…)`
        // plays for `( … )`, which the bare form reads as a group.
        let _kw = input.parse::<kw::brace>()?;
        let content;
        let token = syn::parenthesized!(content in input);
        Ok(Pattern::Braced {
            binding,
            patterns: parse_pattern_list(&content)?,
            token: token::Brace { span: token.span },
        })
    } else if input.peek(token::Paren) {
        let content;
        let token = syn::parenthesized!(content in input);
        Ok(Pattern::Group {
            binding,
            alts: parse_group_content(&content)?,
            token,
        })
    } else if input.peek(kw::recover) {
        let kw_token = input.parse::<kw::recover>()?;
        let content;
        syn::parenthesized!(content in input);
        let body = content.parse()?;
        let _ = content.parse::<Token![,]>()?;
        let sync = content.parse()?;
        Ok(Pattern::Recover {
            binding,
            body: Box::new(body),
            sync: Box::new(sync),
            kw_token,
        })
    } else if input.peek(kw::peek) {
        if binding.is_some() {
            return Err(input.error("Peek cannot be bound."));
        }
        let kw_token = input.parse::<kw::peek>()?;
        let content;
        syn::parenthesized!(content in input);
        let inner = content.parse()?;
        Ok(Pattern::Peek(Box::new(inner), kw_token))
    } else if input.peek(kw::not) {
        if binding.is_some() {
            return Err(input.error("Not cannot be bound."));
        }
        let kw_token = input.parse::<kw::not>()?;
        let content;
        syn::parenthesized!(content in input);
        let inner = content.parse()?;
        Ok(Pattern::Not(Box::new(inner), kw_token))
    } else if input.peek(kw::until) {
        let kw_token = input.parse::<kw::until>()?;
        let content;
        syn::parenthesized!(content in input);
        let pattern = content.parse()?;
        Ok(Pattern::Until {
            binding,
            pattern: Box::new(pattern),
            kw_token,
        })
    } else if input.peek(kw::fold) || input.peek(kw::par_fold) {
        let (kw_span, parallel) = if input.peek(kw::fold) {
            (input.parse::<kw::fold>()?.span, false)
        } else {
            (input.parse::<kw::par_fold>()?.span, true)
        };
        let content;
        syn::parenthesized!(content in input);
        let pattern = content.parse()?;
        let _ = content.parse::<Token![,]>()?;
        let init = Box::new(content.parse::<syn::Expr>()?);
        let _ = content.parse::<Token![,]>()?;
        let step = Box::new(content.parse::<syn::Expr>()?);
        let merge = if parallel {
            let _ = content.parse::<Token![,]>().map_err(|_| {
                content.error("`par_fold` takes a fourth argument, the merge: `par_fold(rule, init, step, merge)`")
            })?;
            Some(Box::new(content.parse::<syn::Expr>()?))
        } else {
            None
        };
        Ok(Pattern::Fold {
            binding,
            pattern: Box::new(pattern),
            init,
            step,
            merge,
            kw_span,
        })
    } else if input.peek(kw::count) {
        let kw_token = input.parse::<kw::count>()?;
        let content;
        syn::parenthesized!(content in input);
        let inner = content.parse()?;
        Ok(Pattern::Count {
            binding,
            pattern: Box::new(inner),
            kw_token,
        })
    } else if input.peek(kw::lex) {
        let kw_token = input.parse::<kw::lex>()?;
        let content;
        syn::parenthesized!(content in input);
        let patterns = parse_pattern_list(&content)?;
        Ok(Pattern::LexicalScope(
            Box::new(Pattern::wrap_sequence(patterns)),
            kw_token,
        ))
    } else if input.peek(kw::spaced) {
        let kw_token = input.parse::<kw::spaced>()?;
        let content;
        syn::parenthesized!(content in input);
        let patterns = parse_pattern_list(&content)?;
        Ok(Pattern::SpacedScope(
            Box::new(Pattern::wrap_sequence(patterns)),
            kw_token,
        ))
    } else if input.peek(kw::fail) {
        if binding.is_some() {
            return Err(input.error("Fail cannot be bound."));
        }
        let kw_token = input.parse::<kw::fail>()?;
        let message = if input.peek(token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            if content.is_empty() {
                None
            } else {
                Some(content.parse()?)
            }
        } else {
            None
        };
        Ok(Pattern::Fail { message, kw_token })
    } else {
        let rule_path = parse_path_no_args(input)?;

        let generics = if input.peek(Token![<]) {
            let _ = input.parse::<Token![<]>()?;
            let mut types = Vec::new();
            if !input.peek(Token![>]) {
                loop {
                    types.push(input.parse::<Type>()?);
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>()?;
                        if input.peek(Token![>]) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            let _gt_token = input.parse::<Token![>]>()?;
            types
        } else {
            Vec::new()
        };

        let args = if !generics.is_empty() {
            if input.peek(token::Paren) {
                parse_args(input)?
            } else {
                Vec::new()
            }
        } else if input.peek(token::Paren) {
            // Simplified Disambiguation logic:
            // 1. `name = value` -> Always allowed (Arguments).
            // 2. Built-in rules -> Always allowed (Arguments).
            // 3. Positional args for user rules -> DISALLOWED (defaults to empty args -> Group).

            let fork = input.fork();
            let content;
            syn::parenthesized!(content in fork);
            let has_named_arg = content.peek(Ident) && content.peek2(Token![=]);

            let is_simple_ident =
                rule_path.segments.len() == 1 && rule_path.leading_colon.is_none();
            let ident_str = if is_simple_ident {
                rule_path.segments[0].ident.to_string()
            } else {
                String::new()
            };
            let is_builtin =
                is_simple_ident && (ident_str == "separated" || ident_str == "repeated");

            // Note: `is_scoped` (e.g. `foo::bar(...)`) is NO LONGER a heuristic for args.
            // Explicitly: only built-ins or named args or templates allowed.

            if has_named_arg || is_builtin {
                parse_args(input)?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Pattern::RuleCall {
            binding,
            rule_path,
            generics,
            args,
        })
    }
}

fn parse_args(input: ParseStream) -> Result<Vec<Argument>> {
    let mut args = Vec::new();
    if input.peek(token::Paren) {
        let content;
        syn::parenthesized!(content in input);
        while !content.is_empty() {
            args.push(content.parse()?);
            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>()?;
            }
        }
    }
    Ok(args)
}

fn parse_pattern_list(input: ParseStream) -> Result<Vec<Pattern>> {
    let mut list = Vec::new();
    while !input.is_empty() {
        list.push(input.parse()?);
    }
    Ok(list)
}

fn parse_group_content(input: ParseStream) -> Result<Vec<GroupAlternative>> {
    let mut alts = Vec::new();
    loop {
        let mut seq = Vec::new();
        while !input.is_empty()
            && !input.peek(Token![|])
            && !input.peek(Token![#])
            && !input.peek(Token![->])
        {
            seq.push(input.parse()?);
        }

        let action = if input.peek(Token![->]) {
            let _: Token![->] = input.parse()?;
            let content;
            syn::braced!(content in input);
            Some(content.parse()?)
        } else {
            None
        };

        let label = if input.peek(Token![#]) {
            let _: Token![#] = input.parse()?;
            let lit: syn::LitStr = input.parse()?;
            Some(lit.value())
        } else {
            None
        };

        alts.push((seq, action, label));
        if input.peek(Token![|]) {
            let _: Token![|] = input.parse()?;
        } else {
            break;
        }
    }
    Ok(alts)
}
