use crate::model::*;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::{parse_quote, Result};

/// Collects all custom keywords from the grammar
pub fn collect_custom_keywords(grammar: &GrammarDefinition) -> HashSet<String> {
    let mut kws = HashSet::new();
    grammar
        .rules
        .iter()
        .flat_map(|r| &r.variants)
        .for_each(|v| collect_from_patterns(&v.pattern, &mut kws));
    kws
}

/// Result of analyzing a pattern sequence for a Cut operator (`=>`)
pub struct CutAnalysis<'a> {
    pub pre_cut: &'a [ModelPattern],
    pub post_cut: &'a [ModelPattern],
}

/// Checks if a sequence contains a Cut operator and splits it.
pub fn find_cut<'a>(patterns: &'a [ModelPattern]) -> Option<CutAnalysis<'a>> {
    let idx = patterns
        .iter()
        .position(|p| matches!(p, ModelPattern::Cut(_)))?;
    Some(CutAnalysis {
        pre_cut: &patterns[0..idx],
        post_cut: &patterns[idx + 1..],
    })
}

/// Splits variants into recursive (starts with the rule name) and base cases.
pub fn split_left_recursive<'a>(
    rule_name: &Ident,
    variants: &'a [RuleVariant],
) -> (Vec<&'a RuleVariant>, Vec<&'a RuleVariant>) {
    let mut recursive = Vec::new();
    let mut base = Vec::new();

    for v in variants {
        if let Some(ModelPattern::RuleCall { rule_name: r, .. }) = v.pattern.first() {
            if r == rule_name {
                recursive.push(v);
                continue;
            }
        }
        base.push(v);
    }
    (recursive, base)
}

fn collect_from_patterns(patterns: &[ModelPattern], kws: &mut HashSet<String>) {
    for p in patterns {
        match p {
            ModelPattern::Lit(lit) => {
                let s = lit.value();
                // Try to tokenize the string literal to find identifiers
                if let Ok(ts) = syn::parse_str::<proc_macro2::TokenStream>(&s) {
                    for token in ts {
                        if let proc_macro2::TokenTree::Ident(ident) = token {
                            let s = ident.to_string();
                            // If syn accepts it as an Ident, it's a candidate.
                            // We rely on syn::parse_str::<syn::Ident> to filter out reserved keywords.
                            // We also exclude "_" because it cannot be a struct name for custom_keyword!.
                            if s != "_" && syn::parse_str::<syn::Ident>(&s).is_ok() {
                                kws.insert(s);
                            }
                        }
                    }
                }
            }
            ModelPattern::Group(alts, _) => {
                alts.iter().for_each(|alt| collect_from_patterns(alt, kws))
            }
            ModelPattern::Bracketed(s, _)
            | ModelPattern::Braced(s, _)
            | ModelPattern::Parenthesized(s, _) => collect_from_patterns(s, kws),
            ModelPattern::Optional(i, _)
            | ModelPattern::Repeat(i, _)
            | ModelPattern::Plus(i, _) => collect_from_patterns(std::slice::from_ref(i), kws),
            ModelPattern::SpanBinding(i, _, _) => {
                collect_from_patterns(std::slice::from_ref(i), kws)
            }
            ModelPattern::Recover { body, sync, .. } => {
                collect_from_patterns(std::slice::from_ref(body), kws);
                collect_from_patterns(std::slice::from_ref(sync), kws);
            }
            ModelPattern::Peek(i, _) | ModelPattern::Not(i, _) => {
                collect_from_patterns(std::slice::from_ref(i), kws)
            }
            _ => {}
        }
    }
}

pub fn collect_bindings(patterns: &[ModelPattern]) -> Vec<Ident> {
    let mut bindings = Vec::new();
    for p in patterns {
        match p {
            ModelPattern::RuleCall {
                binding: Some(b), ..
            } => bindings.push(b.clone()),
            ModelPattern::Repeat(inner, _)
            | ModelPattern::Plus(inner, _)
            | ModelPattern::Optional(inner, _) => {
                bindings.extend(collect_bindings(std::slice::from_ref(inner)));
            }
            ModelPattern::Parenthesized(s, _)
            | ModelPattern::Bracketed(s, _)
            | ModelPattern::Braced(s, _) => {
                bindings.extend(collect_bindings(s));
            }
            ModelPattern::SpanBinding(inner, ident, _) => {
                bindings.push(ident.clone());
                bindings.extend(collect_bindings(std::slice::from_ref(inner)));
            }
            ModelPattern::Recover { binding, body, .. } => {
                if let Some(b) = binding {
                    bindings.push(b.clone());
                } else {
                    bindings.extend(collect_bindings(std::slice::from_ref(body)));
                }
            }
            ModelPattern::Peek(inner, _) => {
                bindings.extend(collect_bindings(std::slice::from_ref(inner)));
            }
            ModelPattern::Group(alts, _) => {
                for alt in alts {
                    bindings.extend(collect_bindings(alt));
                }
            }
            ModelPattern::Not(_, _) => {
                // Not(...) bindings are ignored/dropped because it only succeeds if inner fails.
            }
            _ => {}
        }
    }
    bindings
}

/// Returns the sequence of tokens for syn::parse::<Token>()
///
/// This handles:
/// 1. Custom keywords (e.g. "my_kw")
/// 2. Single tokens (e.g. "->", "==")
/// 3. Multi-token sequences (e.g. "?.", "@detached")
pub fn resolve_token_types(
    lit: &syn::LitStr,
    custom_keywords: &HashSet<String>,
) -> Result<Vec<syn::Type>> {
    let s = lit.value();

    // 1. Check for exact custom keyword match
    if custom_keywords.contains(&s) {
        let ident = format_ident!("{}", s);
        return Ok(vec![parse_quote!(kw::#ident)]);
    }

    // 2. Check for forbidden direct tokens
    if matches!(s.as_str(), "(" | ")" | "[" | "]" | "{" | "}") {
        return Err(syn::Error::new(
            lit.span(),
            format!(
                "Invalid direct token literal: '{}'. Use paren(...), [...] or {{...}} instead.",
                s
            ),
        ));
    }

    // 3. Check for boolean/numeric literals
    if s == "true" || s == "false" {
        return Err(syn::Error::new(
            lit.span(),
            format!(
                "Boolean literal '{}' cannot be used as a token. Use `lit_bool` parser instead.",
                s
            ),
        ));
    }
    if s.chars().next().is_some_and(|c| c.is_numeric()) {
        return Err(syn::Error::new(lit.span(),
            format!("Numeric literal '{}' cannot be used as a token. Use `integer` or `lit_int` parsers instead.", s)));
    }

    // 4. Split into tokens and map each to a Type
    // e.g. "?." -> Token![?] + Token![.]
    // e.g. "@detached" -> Token![@] + kw::detached
    // e.g. "->" -> Token![-] + Token![>] (syn handles this as two Puncts)
    let ts: proc_macro2::TokenStream = syn::parse_str(&s)
        .map_err(|_| syn::Error::new(lit.span(), format!("Invalid token literal: '{}'", s)))?;

    let mut types = Vec::new();
    for token in ts {
        match token {
            proc_macro2::TokenTree::Punct(p) => {
                let c = p.as_char();
                let ty: syn::Type = syn::parse_str(&format!("Token![{}]", c)).map_err(|_| {
                    syn::Error::new(
                        lit.span(),
                        format!("Cannot map punctuation '{}' to Token!", c),
                    )
                })?;
                types.push(ty);
            }
            proc_macro2::TokenTree::Ident(i) => {
                let s = i.to_string();
                if custom_keywords.contains(&s) {
                    let ident = format_ident!("{}", s);
                    types.push(parse_quote!(kw::#ident));
                } else {
                    // Try as standard token (e.g. keyword)
                    let ty: syn::Type =
                        syn::parse_str(&format!("Token![{}]", s)).map_err(|_| {
                            syn::Error::new(
                                lit.span(),
                                format!(
                                "Identifier '{}' is not a custom keyword and not a valid Token!",
                                s
                            ),
                            )
                        })?;
                    types.push(ty);
                }
            }
            _ => {
                return Err(syn::Error::new(
                    lit.span(),
                    "Literal contains unsupported token tree (Group or Literal)",
                ))
            }
        }
    }

    if types.is_empty() {
        return Err(syn::Error::new(
            lit.span(),
            "Empty string literal is not supported.",
        ));
    }

    Ok(types)
}

/// Helper for UPO: Returns a TokenStream for input.peek(...)
pub fn get_simple_peek(
    pattern: &ModelPattern,
    kws: &HashSet<String>,
) -> Result<Option<TokenStream>> {
    match pattern {
        ModelPattern::Lit(lit) => {
            let token_types = resolve_token_types(lit, kws)?;
            // Peek the first token
            if let Some(first_type) = token_types.first() {
                Ok(Some(quote!(#first_type)))
            } else {
                Ok(None)
            }
        }
        ModelPattern::Bracketed(_, _) => Ok(Some(quote!(syn::token::Bracket))),
        ModelPattern::Braced(_, _) => Ok(Some(quote!(syn::token::Brace))),
        ModelPattern::Parenthesized(_, _) => Ok(Some(quote!(syn::token::Paren))),
        ModelPattern::Optional(inner, _)
        | ModelPattern::Repeat(inner, _)
        | ModelPattern::Plus(inner, _) => get_simple_peek(inner, kws),
        ModelPattern::SpanBinding(inner, _, _) => get_simple_peek(inner, kws),
        ModelPattern::Recover { body, .. } => get_simple_peek(body, kws),
        ModelPattern::Group(alts, _) => {
            if alts.len() == 1 {
                if let Some(first) = alts[0].first() {
                    get_simple_peek(first, kws)
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
        ModelPattern::Peek(inner, _) => get_simple_peek(inner, kws),
        ModelPattern::Not(_, _) => Ok(None),
        _ => Ok(None),
    }
}

/// Helper for UPO: Returns a unique string key for the start token
pub fn get_peek_token_string(patterns: &[ModelPattern]) -> Option<String> {
    match patterns.first() {
        Some(ModelPattern::Lit(l)) => Some(l.value()),
        Some(ModelPattern::Bracketed(_, _)) => Some("Bracket".to_string()),
        Some(ModelPattern::Braced(_, _)) => Some("Brace".to_string()),
        Some(ModelPattern::Parenthesized(_, _)) => Some("Paren".to_string()),
        Some(ModelPattern::Optional(inner, _))
        | Some(ModelPattern::Repeat(inner, _))
        | Some(ModelPattern::Plus(inner, _)) => {
            get_peek_token_string(std::slice::from_ref(&**inner))
        }
        Some(ModelPattern::SpanBinding(inner, _, _)) => {
            get_peek_token_string(std::slice::from_ref(&**inner))
        }
        Some(ModelPattern::Recover { body, .. }) => {
            get_peek_token_string(std::slice::from_ref(&**body))
        }
        Some(ModelPattern::Group(alts, _)) => {
            if alts.len() == 1 {
                get_peek_token_string(&alts[0])
            } else {
                None
            }
        }
        Some(ModelPattern::Peek(inner, _)) => get_peek_token_string(std::slice::from_ref(&**inner)),
        Some(ModelPattern::Not(_, _)) => None,
        _ => None,
    }
}

/// Checks if a pattern can match the empty string (epsilon).
/// Used to determine if it is safe to skip a pattern based on a failed peek.
pub fn is_nullable(pattern: &ModelPattern) -> bool {
    match pattern {
        ModelPattern::Cut(_) => true,
        ModelPattern::Lit(_) => false,
        // Conservative assumption: Rule calls might be nullable.
        // To be safe, we assume they are, preventing unsafe peek optimizations.
        ModelPattern::RuleCall { .. } => true,
        ModelPattern::Group(alts, _) => alts.iter().any(|seq| seq.iter().all(is_nullable)),
        ModelPattern::Bracketed(_, _)
        | ModelPattern::Braced(_, _)
        | ModelPattern::Parenthesized(_, _) => false,
        ModelPattern::Optional(_, _) => true,
        ModelPattern::Repeat(_, _) => true,
        ModelPattern::Plus(inner, _) => is_nullable(inner),
        ModelPattern::SpanBinding(inner, _, _) => is_nullable(inner),
        ModelPattern::Recover { .. } => true,
        ModelPattern::Peek(_, _) => true,
        ModelPattern::Not(_, _) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;
    // use syn::spanned::Spanned; // Removed unused import

    #[test]
    fn test_resolve_token_types_valid() {
        let kws = HashSet::new();
        let lit: syn::LitStr = parse_quote!("fn");
        let types = resolve_token_types(&lit, &kws).unwrap();
        assert_eq!(types.len(), 1);
    }

    #[test]
    fn test_resolve_token_types_invalid_direct() {
        let kws = HashSet::new();
        let lit: syn::LitStr = parse_quote!("(");
        let err = resolve_token_types(&lit, &kws).unwrap_err();
        assert!(err.to_string().contains("Invalid direct token literal"));
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", lit.span()));
    }

    #[test]
    fn test_resolve_token_types_invalid_bool() {
        let kws = HashSet::new();
        let lit: syn::LitStr = parse_quote!("true");
        let err = resolve_token_types(&lit, &kws).unwrap_err();
        assert!(err.to_string().contains("Boolean literal"));
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", lit.span()));
    }

    #[test]
    fn test_resolve_token_types_invalid_numeric() {
        let kws = HashSet::new();
        let lit: syn::LitStr = parse_quote!("123");
        let err = resolve_token_types(&lit, &kws).unwrap_err();
        assert!(err.to_string().contains("Numeric literal"));
        assert_eq!(format!("{:?}", err.span()), format!("{:?}", lit.span()));
    }
}
