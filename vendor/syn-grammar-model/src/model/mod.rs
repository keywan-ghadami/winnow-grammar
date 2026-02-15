// Moved from macros/src/model.rs
pub mod backend;
pub mod types;

pub use backend::*;
pub use types::*;

use crate::parser;
use proc_macro2::{Span, TokenStream};
use syn::spanned::Spanned as _;
use syn::{Attribute, Ident, ItemUse, Lit, LitStr, Type};

#[derive(Debug, Clone)]
pub struct GrammarDefinition {
    pub name: Ident,
    pub inherits: Option<Ident>,
    pub uses: Vec<ItemUse>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub attrs: Vec<Attribute>,
    pub is_pub: bool,
    pub name: Ident,
    pub params: Vec<(Ident, Type)>,
    pub return_type: Type,
    pub variants: Vec<RuleVariant>,
}

#[derive(Debug, Clone)]
pub struct RuleVariant {
    pub pattern: Vec<ModelPattern>,
    pub action: TokenStream,
}

#[derive(Debug, Clone)]
pub enum ModelPattern {
    Cut(Span),
    Lit(LitStr),
    RuleCall {
        binding: Option<Ident>,
        rule_name: Ident,
        args: Vec<Lit>,
    },
    Group(Vec<Vec<ModelPattern>>, Span),
    Bracketed(Vec<ModelPattern>, Span),
    Braced(Vec<ModelPattern>, Span),
    Parenthesized(Vec<ModelPattern>, Span),
    Optional(Box<ModelPattern>, Span),
    Repeat(Box<ModelPattern>, Span),
    Plus(Box<ModelPattern>, Span),
    SpanBinding(Box<ModelPattern>, Ident, Span),
    Recover {
        binding: Option<Ident>,
        body: Box<ModelPattern>,
        sync: Box<ModelPattern>,
        span: Span,
    },
    Peek(Box<ModelPattern>, Span),
    Not(Box<ModelPattern>, Span),
}

impl From<parser::GrammarDefinition> for GrammarDefinition {
    fn from(p: parser::GrammarDefinition) -> Self {
        Self {
            name: p.name,
            inherits: p.inherits.map(|spec| spec.name),
            uses: p.uses,
            rules: p.rules.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<parser::Rule> for Rule {
    fn from(p: parser::Rule) -> Self {
        Self {
            attrs: p.attrs,
            is_pub: p.is_pub.is_some(),
            name: p.name,
            params: p
                .params
                .into_iter()
                .map(|param| (param.name, param.ty))
                .collect(),
            return_type: p.return_type,
            variants: p.variants.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<parser::RuleVariant> for RuleVariant {
    fn from(p: parser::RuleVariant) -> Self {
        Self {
            pattern: p.pattern.into_iter().map(Into::into).collect(),
            action: p.action,
        }
    }
}

impl From<parser::Pattern> for ModelPattern {
    fn from(p: parser::Pattern) -> Self {
        use parser::Pattern as P;
        match p {
            P::Cut(t) => ModelPattern::Cut(t.span()),
            P::Lit(l) => ModelPattern::Lit(l),
            P::RuleCall {
                binding,
                rule_name,
                args,
            } => ModelPattern::RuleCall {
                binding,
                rule_name,
                args,
            },
            P::Group(alts, token) => ModelPattern::Group(
                alts.into_iter()
                    .map(|seq| seq.into_iter().map(ModelPattern::from).collect())
                    .collect(),
                token.span.join(),
            ),
            P::Bracketed(p, token) => ModelPattern::Bracketed(
                p.into_iter().map(ModelPattern::from).collect(),
                token.span.join(),
            ),
            P::Braced(p, token) => ModelPattern::Braced(
                p.into_iter().map(ModelPattern::from).collect(),
                token.span.join(),
            ),
            P::Parenthesized(p, _, token) => ModelPattern::Parenthesized(
                p.into_iter().map(ModelPattern::from).collect(),
                token.span.join(),
            ),
            P::Optional(p, token) => {
                ModelPattern::Optional(Box::new(ModelPattern::from(*p)), token.span())
            }
            P::Repeat(p, token) => {
                ModelPattern::Repeat(Box::new(ModelPattern::from(*p)), token.span())
            }
            P::Plus(p, token) => ModelPattern::Plus(Box::new(ModelPattern::from(*p)), token.span()),
            P::SpanBinding(p, ident, token) => {
                ModelPattern::SpanBinding(Box::new(ModelPattern::from(*p)), ident, token.span)
            }
            P::Recover {
                binding,
                body,
                sync,
                kw_token,
            } => ModelPattern::Recover {
                binding,
                body: Box::new(ModelPattern::from(*body)),
                sync: Box::new(ModelPattern::from(*sync)),
                span: kw_token.span(),
            },
            P::Peek(p, token) => ModelPattern::Peek(Box::new(ModelPattern::from(*p)), token.span()),
            P::Not(p, token) => ModelPattern::Not(Box::new(ModelPattern::from(*p)), token.span()),
        }
    }
}

impl ModelPattern {
    pub fn span(&self) -> Span {
        match self {
            ModelPattern::Cut(s) => *s,
            ModelPattern::Lit(l) => l.span(),
            ModelPattern::RuleCall { rule_name, .. } => rule_name.span(),
            ModelPattern::Optional(_, s)
            | ModelPattern::Repeat(_, s)
            | ModelPattern::Plus(_, s) => *s,
            ModelPattern::SpanBinding(_, _, s) => *s,
            ModelPattern::Recover { span, .. } => *span,
            ModelPattern::Group(_, s) => *s,
            ModelPattern::Bracketed(_, s)
            | ModelPattern::Braced(_, s)
            | ModelPattern::Parenthesized(_, s) => *s,
            ModelPattern::Peek(_, s) | ModelPattern::Not(_, s) => *s,
        }
    }
}
