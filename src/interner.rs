use lasso::{Key, Spur, ThreadedRodeo};
use std::num::NonZeroU32;
use std::sync::Arc;

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
}

#[derive(Debug, Clone)]
pub struct InternerContext {
    backend: Arc<ThreadedRodeo>,
}

impl InternerContext {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(ThreadedRodeo::default()),
        }
    }

    pub fn intern_string(&self, text: &str) -> Symbol {
        let spur = self.backend.get_or_intern(text);
        Symbol::from_spur(spur)
    }

    pub fn resolve(&self, symbol: Symbol) -> &str {
        self.backend.resolve(&symbol.into_spur())
    }

    /// How many distinct strings the interner holds - which is also one past
    /// the largest [`Symbol::index`] it has handed out, so it is the size a
    /// caller's parallel `Vec` needs.
    pub fn len(&self) -> usize {
        self.backend.len()
    }

    /// Whether nothing has been interned yet.
    pub fn is_empty(&self) -> bool {
        self.backend.is_empty()
    }
}

impl Default for InternerContext {
    fn default() -> Self {
        Self::new()
    }
}
