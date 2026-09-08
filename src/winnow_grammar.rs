#![doc = include_str!("../README.md")]
#![doc = "\n\n"]
#![doc = include_str!("../SYNTAX.md")]

// src/lib.rs

// Re-export the macro
pub use winnow_grammar_macros::grammar;

// Re-export winnow so generated code has access to it
pub use winnow;

/// The error type of the generated parsers and the selection between errors.
pub mod error;
#[doc(hidden)]
pub mod intern_cache;
pub mod interner;
/// Runtime helpers for the generated code.
pub mod rt;
pub mod span;
pub mod state;
pub mod test_result;
pub mod testing;

pub use error::{
    Diagnostics, ParseError, PRIO_AGGREGATED, PRIO_LABELED, PRIO_NORMAL, PRIO_STRUCTURAL,
};

pub use interner::{InternerContext, Symbol};
pub use state::StateOf;

/// When and how a failing parse produces its diagnostics.
///
/// Every generated entry point - `parse_<rule>()` and, per piece,
/// `parse_<rule>_pieces()` - first runs a **fast pass** with a zero-sized
/// error type: no expectations, no positions, no rule stacks are built while
/// it runs. Only when it fails is the input parsed a second time, in
/// **diagnose mode**: the full engine of ADR 15, and its error is the one
/// reported. On a `par_fold` rule the second pass starts at the item the
/// first one stopped in, not at the beginning. ADR 17 has the reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Diagnose {
    /// Fast pass; on failure restore `user_state` from a clone taken before
    /// the pass, then replay in diagnose mode. An action that mutated the
    /// state during the fast pass runs again, on the restored state - once,
    /// as seen from the outside.
    #[default]
    Replay,
    /// Fast pass; on failure replay on the same, already mutated context.
    /// Saves the clone; an action that mutates `user_state` runs twice.
    ReplayInPlace,
    /// Fast pass only. A failure is just a failure: no second pass, no
    /// position, no message - the error is [`ParseError::undiagnosed`]. For
    /// a caller that needs the verdict, not the reason.
    Off,
    /// No fast pass: diagnose straight away. What every parse did before
    /// ADR 17, and the reference a test compares the replay against.
    Eager,
}

/// Where the fold in the body of a `par_fold` rule stopped, and where its
/// item numbering starts.
///
/// The fold writes `seen` and `at` on the way out; the replay of a failed
/// fast pass skips to `at` and numbers its items from `base` - see
/// [`crate::rt::entry_framed`]. Two words written per failure, nothing per
/// item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FoldProgress {
    /// Items before this input: a replay of a tail numbers from here.
    pub base: usize,
    /// Items the fold accepted before it stopped.
    pub seen: usize,
    /// Byte offset of the item the fold stopped in.
    pub at: usize,
}

/// The shared context that is passed as state to the parser.
///
/// It contains the thread-safe string interner and a placeholder for any user-defined state.
#[derive(Debug, Clone)]
pub struct ParseContext<S = ()> {
    /// A thread-safe, shared string interner.
    pub interner: InternerContext,
    /// A lookup cache in front of the interner, private to this context and
    /// therefore to this parse - and, under a `par_fold`, to this piece. It
    /// holds no authority: a miss goes to the interner, which decides.
    ///
    /// An implementation detail that has to be visible because callers build
    /// this struct with a literal; `..Default::default()` fills it and nothing
    /// else should touch it. See `TODO.md` §4.
    #[doc(hidden)]
    pub intern_cache: intern_cache::InternCache,
    /// A placeholder for user-defined state.
    pub user_state: S,
    /// The furthest failure position that a successful backtrack (`x?`, `x*`)
    /// discarded. Compared against the returned error at the end - see
    /// [`crate::rt::finish`].
    pub furthest: Option<ParseError>,
    /// The **live** rule stack: the rules currently running, outermost first.
    /// An error that is passed out collects its rules itself on the way back;
    /// an error that is *recorded* along the way never gets there - it
    /// receives the outer rules from here.
    pub rules: Vec<&'static str>,
    /// When a failing parse is diagnosed - see [`Diagnose`].
    pub diagnose: Diagnose,
    /// How often `recover(…)` swallowed a failure and skipped to its
    /// synchronisation point. Counted in both passes, so it is there after a
    /// parse that succeeded - which a parse with recoveries does.
    pub recoveries: usize,
    /// What those recoveries swallowed, when the diagnosing engine was the one
    /// running. The fast pass has no error to keep (its error type is
    /// zero-sized), so this is empty after a parse that only ran it: the count
    /// above says *that* something was recovered, `Diagnose::Eager` says
    /// *what*. See `TODO.md` §2.
    pub recovered: Vec<ParseError>,
    /// Where the fold of a `par_fold` rule stopped - see [`FoldProgress`].
    pub fold: FoldProgress,
}

impl<S: Default> Default for ParseContext<S> {
    fn default() -> Self {
        Self {
            interner: InternerContext::new(),
            intern_cache: intern_cache::InternCache::new(),
            user_state: S::default(),
            furthest: None,
            rules: Vec::new(),
            diagnose: Diagnose::default(),
            recoveries: 0,
            recovered: Vec::new(),
            fold: FoldProgress::default(),
        }
    }
}

impl<S> ParseContext<S> {
    /// A context around a user state, with a fresh interner and the default
    /// diagnostics settings.
    ///
    /// [`Default`] cannot serve here: it requires `S: Default`, which a state
    /// that carries a pre-sized table or a handle need not be. Naming every
    /// field requires nothing of `S` - see ADR 20.
    pub fn with_state(user_state: S) -> Self {
        Self {
            interner: InternerContext::new(),
            intern_cache: intern_cache::InternCache::new(),
            user_state,
            furthest: None,
            rules: Vec::new(),
            diagnose: Diagnose::default(),
            recoveries: 0,
            recovered: Vec::new(),
            fold: FoldProgress::default(),
        }
    }

    /// [`with_state`](Self::with_state) with an interner the caller already
    /// has - the shape ADR 14 asks for when one interner outlives the parse,
    /// and what the pieces of a `par_fold` share.
    pub fn with_state_and_interner(user_state: S, interner: InternerContext) -> Self {
        Self {
            interner,
            ..Self::with_state(user_state)
        }
    }

    /// Takes ownership of the fields that belong to *one* parse, so that a
    /// context can be used for a second.
    ///
    /// The interner and the user state are the caller's and outlive the
    /// parse, which is what ADR 14 built this struct for. `furthest`,
    /// `rules` and `fold` are the diagnostics engine's working space and
    /// belong to the parse that is starting. Nothing reset them, so
    /// `fold.base` (which numbers a `par_fold`'s items, and is advanced when
    /// one fails) carried into the next parse and numbered its items from
    /// the previous total.
    ///
    /// Called by [`rt::entry`] and
    /// [`rt::entry_framed`], which is where a parse
    /// begins. Nesting one inside the other would reset the outer one's
    /// state, and does not happen: [`rt::finish`] fails a
    /// parse with input left over, so an entry point called inside another
    /// parse already fails.
    pub fn begin_parse(&mut self) {
        self.furthest = None;
        self.rules.clear();
        self.fold = FoldProgress::default();
        self.recoveries = 0;
        self.recovered.clear();
        self.intern_cache.rebind(&self.interner);
    }

    /// Records what a `recover(…)` swallowed: always the count, and the error
    /// itself when the diagnosing engine produced one.
    pub fn record_recovery(&mut self, error: Option<ParseError>) {
        self.recoveries += 1;
        if let Some(e) = error {
            self.recovered.push(e);
        }
    }

    /// The symbol for `text`, through this context's cache.
    ///
    /// What `ident` and `intern(…)` call, and what an action should call
    /// instead of `_state.interner.intern_string(…)`: the interner is correct
    /// either way, this one is faster on the words a parse sees more than once
    /// (`TODO.md` §4).
    #[inline]
    pub fn intern(&mut self, text: &str) -> Symbol {
        self.intern_cache.intern(&self.interner, text)
    }

    /// Records a discarded error - following the same ranking as
    /// [`ParseError::merge`].
    pub fn record(&mut self, e: &ParseError) {
        let mut e = e.clone();
        for r in self.rules.iter().rev() {
            e.push_rule(r);
        }
        self.furthest = Some(match self.furthest.take() {
            Some(f) => f.merge(e),
            None => e,
        });
    }

    /// The better of the returned error and the recorded one.
    pub fn best(&self, e: ParseError) -> ParseError {
        match &self.furthest {
            Some(f) => f.clone().merge(e),
            None => e,
        }
    }
}

/// The input stream type used by all parsers generated by the `grammar!` macro.
///
/// It combines the input string slice with the shared `ParseContext`.
pub type ParseInput<'a, S = ()> =
    ::winnow::stream::Stateful<::winnow::stream::LocatingSlice<&'a str>, ParseContext<S>>;

pub mod types {
    use proc_macro2::TokenStream;
    use quote::ToTokens;
    use std::fmt;
    use std::hash::{Hash, Hasher};

    pub use proc_macro2::Span;

    /// Constructs a value from parsed data and the position where it was found.
    ///
    /// Implemented by the attribute macro [`with_span`](winnow_grammar_macros::with_span).
    /// Formerly obtained from `grammar-kit`; moved here during the move out of
    /// the syn-grammar monorepo so that `winnow-grammar` no longer needs a
    /// dependency on the syn-side runtime.
    pub trait WithSpan<ParsedData> {
        /// Builds `Self` from `parsed_data` and the byte range `span`.
        fn with_span(parsed_data: ParsedData, span: std::ops::Range<usize>) -> Self;
    }

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
