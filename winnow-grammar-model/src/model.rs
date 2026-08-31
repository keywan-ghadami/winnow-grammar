use crate::{analysis, parser};
use proc_macro2::{Ident, TokenStream};
use syn::spanned::Spanned;

pub mod backend;

#[derive(Debug, Clone)]
pub struct GrammarDefinition {
    pub name: Ident,
    pub rules: Vec<Rule>,
    pub extern_rules: Vec<ExternRule>,
    pub imports: Vec<ImportedGrammar>,
    pub uses: Vec<syn::ItemUse>,
}

#[derive(Debug, Clone)]
pub struct ExternRule {
    pub name: Ident,
    pub generics: syn::Generics,
    pub params: Vec<RuleParameter>,
    pub return_type: syn::Type,
    pub attrs: Vec<syn::Attribute>,
}

#[derive(Debug, Clone)]
pub struct ImportedGrammar {
    pub path: syn::Path,
    pub alias: Ident,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub name: Ident,
    pub generics: syn::Generics,
    pub params: Vec<RuleParameter>,
    pub return_type: syn::Type,
    pub return_type_kind: analysis::ReturnTypeKind,
    pub variants: Vec<RuleVariant>,
    pub is_pub: bool,
    pub is_lexical: bool,
    pub attrs: Vec<syn::Attribute>,
}

#[derive(Debug, Clone)]
pub struct RuleParameter {
    pub name: Ident,
    pub ty: Option<syn::Type>,
}

#[derive(Debug, Clone)]
pub struct RuleVariant {
    pub pattern: Vec<ModelPattern>,
    pub action: TokenStream,
    pub label: Option<String>,
    pub with_span: bool,
    pub is_explicit: bool,
}

#[derive(Debug, Clone)]
pub enum ModelPattern {
    Cut(proc_macro2::Span),
    Lit {
        binding: Option<Ident>,
        lit: syn::Lit,
    },
    RuleCall {
        binding: Option<Ident>,
        rule_path: syn::Path,
        generics: Vec<syn::Type>,
        args: Vec<Argument>,
    },
    Group {
        binding: Option<Ident>,
        alts: Vec<(Vec<ModelPattern>, Option<TokenStream>, Option<String>)>,
        span: proc_macro2::Span,
    },
    Bracketed(Vec<ModelPattern>, proc_macro2::Span),
    Braced(Vec<ModelPattern>, proc_macro2::Span),
    Parenthesized(Vec<ModelPattern>, proc_macro2::Span),
    Optional(Box<ModelPattern>, proc_macro2::Span),
    Repeat(Box<ModelPattern>, proc_macro2::Span),
    Plus(Box<ModelPattern>, proc_macro2::Span),
    SpanBinding(Box<ModelPattern>, Ident, proc_macro2::Span),
    Recover {
        binding: Option<Ident>,
        body: Box<ModelPattern>,
        sync: Box<ModelPattern>,
        span: proc_macro2::Span,
    },
    Peek(Box<ModelPattern>, proc_macro2::Span),
    Not(Box<ModelPattern>, proc_macro2::Span),
    Until {
        binding: Option<Ident>,
        pattern: Box<ModelPattern>,
        span: proc_macro2::Span,
    },
    Count {
        binding: Option<Ident>,
        pattern: Box<ModelPattern>,
        span: proc_macro2::Span,
    },
    LexicalScope(Box<ModelPattern>, proc_macro2::Span),
    SpacedScope(Box<ModelPattern>, proc_macro2::Span),
    Fail {
        message: Option<syn::Lit>,
        span: proc_macro2::Span,
    },
}

impl ModelPattern {
    pub fn span(&self) -> proc_macro2::Span {
        match self {
            ModelPattern::Cut(s) => *s,
            ModelPattern::Lit { lit, .. } => lit.span(),
            ModelPattern::RuleCall { rule_path, .. } => {
                use syn::spanned::Spanned;
                rule_path.span()
            }
            ModelPattern::Group { span, .. } => *span,
            ModelPattern::Bracketed(_, s) => *s,
            ModelPattern::Braced(_, s) => *s,
            ModelPattern::Parenthesized(_, s) => *s,
            ModelPattern::Optional(_, s) => *s,
            ModelPattern::Repeat(_, s) => *s,
            ModelPattern::Plus(_, s) => *s,
            ModelPattern::SpanBinding(_, _, s) => *s,
            ModelPattern::Recover { span, .. } => *span,
            ModelPattern::Peek(_, s) => *s,
            ModelPattern::Not(_, s) => *s,
            ModelPattern::Until { span, .. } => *span,
            ModelPattern::Count { span, .. } => *span,
            ModelPattern::LexicalScope(_, s) => *s,
            ModelPattern::SpacedScope(_, s) => *s,
            ModelPattern::Fail { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Argument {
    Positional(ModelPattern),
    Named(Ident, ModelPattern),
}

impl From<parser::GrammarDefinition> for GrammarDefinition {
    fn from(p: parser::GrammarDefinition) -> Self {
        let mut uses = p.uses;
        if let Some(inherits) = p.inherits {
            // Deprecation warning could be emitted here if we had a way to report it
            // For now, we just map it to a use super::*; for compatibility
            let name = inherits.name;
            let item_use: syn::ItemUse = syn::parse_quote!(use super::#name::*;);
            uses.insert(0, item_use);
        }
        GrammarDefinition {
            name: p.name,
            rules: p.rules.into_iter().map(Into::into).collect(),
            extern_rules: p.extern_rules.into_iter().map(Into::into).collect(),
            imports: p.imports.into_iter().map(Into::into).collect(),
            uses,
        }
    }
}

impl From<parser::ExternRule> for ExternRule {
    fn from(p: parser::ExternRule) -> Self {
        ExternRule {
            name: p.name,
            generics: p.generics,
            params: p.params.into_iter().map(Into::into).collect(),
            return_type: p.return_type,
            attrs: p.attrs,
        }
    }
}

impl From<parser::ImportedGrammar> for ImportedGrammar {
    fn from(p: parser::ImportedGrammar) -> Self {
        ImportedGrammar {
            path: p.path,
            alias: p.alias,
        }
    }
}

impl From<parser::Rule> for Rule {
    fn from(p: parser::Rule) -> Self {
        let is_lexical = p
            .name
            .to_string()
            .chars()
            .next()
            .is_some_and(char::is_uppercase);
        let return_type_kind = analysis::determine_return_type_kind(&p.return_type);
        Rule {
            name: p.name,
            generics: p.generics,
            params: p.params.into_iter().map(Into::into).collect(),
            return_type: p.return_type,
            return_type_kind,
            variants: p.variants.into_iter().map(Into::into).collect(),
            is_pub: p.is_pub.is_some(),
            is_lexical,
            attrs: p.attrs,
        }
    }
}

impl From<parser::RuleParameter> for RuleParameter {
    fn from(p: parser::RuleParameter) -> Self {
        RuleParameter {
            name: p.name,
            ty: p.ty,
        }
    }
}

impl From<parser::RuleVariant> for RuleVariant {
    fn from(p: parser::RuleVariant) -> Self {
        RuleVariant {
            pattern: p.pattern.into_iter().map(Into::into).collect(),
            action: p.action,
            label: p.label,
            with_span: p.with_span,
            is_explicit: p.is_explicit,
        }
    }
}

impl From<parser::Pattern> for ModelPattern {
    fn from(p: parser::Pattern) -> Self {
        match p {
            parser::Pattern::Cut(token) => ModelPattern::Cut(token.span()), // FatArrow has .span()
            parser::Pattern::Lit { binding, lit } => ModelPattern::Lit { binding, lit },
            parser::Pattern::RuleCall {
                binding,
                rule_path,
                generics,
                args,
            } => ModelPattern::RuleCall {
                binding,
                rule_path,
                generics,
                args: args.into_iter().map(Into::into).collect(),
            },
            parser::Pattern::Group {
                binding,
                alts,
                token,
            } => ModelPattern::Group {
                binding,
                alts: alts
                    .into_iter()
                    .map(|(seq, action, label)| {
                        (seq.into_iter().map(Into::into).collect(), action, label)
                    })
                    .collect(),
                span: token.span.join(), // Paren has .span: DelimSpan which has .join() -> Span
            },
            parser::Pattern::Bracketed {
                binding: _,
                patterns,
                token,
            } => {
                // Ignoring binding for Bracketed as not supported in ModelPattern yet
                ModelPattern::Bracketed(
                    patterns.into_iter().map(Into::into).collect(),
                    token.span.join(),
                )
            }
            parser::Pattern::Braced {
                binding: _,
                patterns,
                token,
            } => {
                // Ignoring binding for Braced as not supported in ModelPattern yet
                ModelPattern::Braced(
                    patterns.into_iter().map(Into::into).collect(),
                    token.span.join(),
                )
            }
            parser::Pattern::Parenthesized {
                binding: _,
                patterns,
                kw_token: _,
                token,
            } => {
                // Ignoring binding for Parenthesized as not supported in ModelPattern yet
                ModelPattern::Parenthesized(
                    patterns.into_iter().map(Into::into).collect(),
                    token.span.join(),
                )
            }
            parser::Pattern::Optional(p, token) => {
                ModelPattern::Optional(Box::new((*p).into()), token.span)
            } // Question has .span (field)
            parser::Pattern::Repeat(p, token) => {
                ModelPattern::Repeat(Box::new((*p).into()), token.span)
            } // Star has .span (field)
            parser::Pattern::Plus(p, token) => {
                ModelPattern::Plus(Box::new((*p).into()), token.span)
            } // Plus has .span (field)
            parser::Pattern::SpanBinding(p, id, token) => {
                ModelPattern::SpanBinding(Box::new((*p).into()), id, token.span)
                // At has .span (field)
            }
            parser::Pattern::Recover {
                binding,
                body,
                sync,
                kw_token,
            } => ModelPattern::Recover {
                binding,
                body: Box::new((*body).into()),
                sync: Box::new((*sync).into()),
                span: kw_token.span(), // Custom Keyword has .span()
            },
            parser::Pattern::Peek(p, token) => {
                ModelPattern::Peek(Box::new((*p).into()), token.span())
            } // Custom Keyword has .span()
            parser::Pattern::Not(p, token) => {
                ModelPattern::Not(Box::new((*p).into()), token.span())
            } // Custom Keyword has .span()
            parser::Pattern::Until {
                binding,
                pattern,
                kw_token,
            } => ModelPattern::Until {
                binding,
                pattern: Box::new((*pattern).into()),
                span: kw_token.span(), // Custom Keyword has .span()
            },
            parser::Pattern::Count {
                binding,
                pattern,
                kw_token,
            } => ModelPattern::Count {
                binding,
                pattern: Box::new((*pattern).into()),
                span: kw_token.span(),
            },
            parser::Pattern::LexicalScope(pattern, kw_token) => {
                ModelPattern::LexicalScope(Box::new((*pattern).into()), kw_token.span())
            }
            parser::Pattern::SpacedScope(pattern, kw_token) => {
                ModelPattern::SpacedScope(Box::new((*pattern).into()), kw_token.span())
            }
            parser::Pattern::Fail { message, kw_token } => ModelPattern::Fail {
                message,
                span: kw_token.span(),
            },
        }
    }
}

impl From<parser::Argument> for Argument {
    fn from(p: parser::Argument) -> Self {
        match p {
            parser::Argument::Positional(p) => Argument::Positional(p.into()),
            parser::Argument::Named(n, p) => Argument::Named(n, p.into()),
        }
    }
}
