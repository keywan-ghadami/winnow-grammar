#![doc = include_str!("../README.md")]
#![doc = "\n\n"]
#![doc = include_str!("../SYNTAX.md")]

// src/lib.rs

// Re-export the macro
pub use winnow_grammar_macros::grammar;

// Re-export winnow so generated code has access to it
pub use winnow;

pub mod interner;
pub use interner::{InternerContext, Symbol};

pub type ParseInput<'a, S = InternerContext> =
    ::winnow::stream::Stateful<::winnow::stream::LocatingSlice<&'a str>, S>;

// Re-export testing utilities
pub mod testing;

/// Portable types for backend compatibility
pub mod types {
    use proc_macro2::TokenStream;
    use quote::ToTokens;
    use std::fmt;
    use std::hash::{Hash, Hasher};

    pub use grammar_kit::WithSpan;
    pub use proc_macro2::Span;

    /// A generic wrapper that attaches a source span to a value.
    #[derive(Clone, Copy)]
    pub struct SpannedValue<T> {
        pub value: T,
        pub span: Span,
    }

    impl<T> SpannedValue<T> {
        pub fn new(value: T, span: Span) -> Self {
            Self { value, span }
        }
    }

    impl<T: PartialEq> PartialEq for SpannedValue<T> {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    impl<T: Eq> Eq for SpannedValue<T> {}

    impl<T: Hash> Hash for SpannedValue<T> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.value.hash(state);
        }
    }

    impl<T: fmt::Display> fmt::Display for SpannedValue<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.value.fmt(f)
        }
    }

    impl<T: fmt::Debug> fmt::Debug for SpannedValue<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SpannedValue")
                .field("value", &self.value)
                .field("span", &self.span)
                .finish()
        }
    }

    impl<T: ToTokens> ToTokens for SpannedValue<T>
    where
        T: ToTokens,
    {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            self.value.to_tokens(tokens);
        }
    }
}

pub use types::{SpannedValue, WithSpan};
