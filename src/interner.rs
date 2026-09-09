use lasso::{Key, Spur, ThreadedRodeo};

/// How the interner hashes. `ahash` under its feature, std's `RandomState`
/// otherwise - see `InternerContext`.
#[cfg(feature = "ahash")]
type Hasher = ahash::RandomState;
#[cfg(not(feature = "ahash"))]
type Hasher = std::hash::RandomState;
use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Symbol(NonZeroU32);

impl Symbol {
    // `Symbol` stores `index + 1` so the `NonZeroU32` niche stays valid and
    // `Option<Symbol>` is still four bytes.
    //
    // Both directions go through `Key`'s *index* representation. Mixing it with
    // `Spur::into_inner()` - which yields the raw key, already `index + 1` - is
    // what made the round-trip add one twice.

    #[doc(hidden)]
    pub fn from_spur(spur: lasso::Spur) -> Self {
        let index = spur.into_usize() as u32;
        Self(NonZeroU32::new(index + 1).expect("index + 1 is never zero"))
    }

    #[doc(hidden)]
    pub fn into_spur(self) -> Spur {
        let index = self.0.get() as usize - 1;
        Spur::try_from_usize(index).expect("Invalid Symbol ID")
    }

    /// The symbol's position in its interner: **dense**, zero-based, and
    /// assigned in the order the interner first saw each string.
    ///
    /// This is what makes a symbol useful as more than an identity. The `n`
    /// distinct strings an interner holds have the indices `0..n`, so a caller
    /// can put its own data in a `Vec` and reach it in one step:
    ///
    /// ```
    /// use winnow_grammar::InternerContext;
    ///
    /// let interner = InternerContext::new();
    /// let mut totals: Vec<i64> = Vec::new();
    ///
    /// for (city, temp) in [("Hamburg", 12), ("Zürich", 20), ("Hamburg", 8)] {
    ///     let sym = interner.intern_string(city);
    ///     let i = sym.index() as usize;
    ///     if i >= totals.len() {
    ///         totals.resize(i + 1, 0);
    ///     }
    ///     totals[i] += temp;
    /// }
    ///
    /// assert_eq!(totals.len(), interner.len());
    /// assert_eq!(totals[interner.intern_string("Hamburg").index() as usize], 20);
    /// ```
    ///
    /// What the index is **not**: a value that means anything outside the
    /// interner that produced it. It depends on the order strings were first
    /// seen, so it differs between two runs over different input, between two
    /// interners over the same input, and - when the pieces of a `par_fold`
    /// share one interner - on how the threads interleaved. Do not persist it,
    /// do not compare it across interners, and do not rely on its order
    /// carrying meaning. To recover the text, use
    /// [`InternerContext::resolve`].
    pub fn index(self) -> u32 {
        self.0.get() - 1
    }

    /// The inverse of [`index`](Self::index). Deliberately not public: a
    /// symbol built from a number nothing interned resolves to whatever
    /// happens to be at that index, or panics. The cache uses it to rebuild
    /// what it stored.
    pub(crate) fn from_index(index: u32) -> Self {
        Self(NonZeroU32::new(index + 1).expect("index + 1 is never zero"))
    }
}

/// The interner a parse interns into: a shared `lasso::ThreadedRodeo` behind
/// an `Arc`, which ADR 14 makes the caller's to own and to share.
///
/// **The hash function is a cargo feature, not a type parameter.** With
/// `ahash` on, it hashes with `ahash::RandomState`; otherwise with std's.
/// Both are seeded per process. Making it a type parameter instead would put
/// one in `ParseContext` and from there in every generated signature, for a
/// choice that has two sensible answers - so it is a feature, and the crate
/// does not ask a grammar author about it.
///
/// A caller who wants an interner of a different *kind* - a slot table whose
/// numbers are the identity, say - does not replace this one: they declare a
/// `state` and put it there (ADR 20). `ident` and `intern(…)` keep meaning
/// this interner, which is what a built-in is for.
///
/// **The map behind it is built on first use, not on `new()`.** A
/// `ThreadedRodeo` is sharded and allocates every shard: constructing one
/// measured 1.4 µs, which is what building a `ParseContext` used to cost
/// almost entirely (`benches/context.rs`). A grammar that never writes `ident`
/// or `intern(…)` never interns, and should not pay for a map it will not
/// read - so it does not. The check is one relaxed atomic load on the interner
/// path, which is reached only when the context's lookup cache misses.
#[derive(Debug, Clone)]
pub struct InternerContext {
    backend: Arc<OnceLock<ThreadedRodeo<Spur, Hasher>>>,
}

impl InternerContext {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(OnceLock::new()),
        }
    }

    /// The map, built if this is the first time anyone asked.
    #[inline]
    fn rodeo(&self) -> &ThreadedRodeo<Spur, Hasher> {
        self.backend
            .get_or_init(|| ThreadedRodeo::with_hasher(Hasher::default()))
    }

    pub fn intern_string(&self, text: &str) -> Symbol {
        let spur = self.rodeo().get_or_intern(text);
        Symbol::from_spur(spur)
    }

    pub fn resolve(&self, symbol: Symbol) -> &str {
        self.rodeo().resolve(&symbol.into_spur())
    }

    /// How many distinct strings the interner holds - which is also one past
    /// the largest [`Symbol::index`] it has handed out, so it is the size a
    /// caller's parallel `Vec` needs.
    pub fn len(&self) -> usize {
        self.backend.get().map_or(0, |r| r.len())
    }

    /// Whether nothing has been interned yet.
    pub fn is_empty(&self) -> bool {
        self.backend.get().is_none_or(|r| r.is_empty())
    }

    /// Identifies the interner *behind* this handle: two clones of one
    /// interner share it, two separate ones do not. Used to tell whether a
    /// cache of symbols still belongs to the interner it was filled from.
    pub(crate) fn id(&self) -> usize {
        Arc::as_ptr(&self.backend) as usize
    }
}

impl Default for InternerContext {
    fn default() -> Self {
        Self::new()
    }
}
