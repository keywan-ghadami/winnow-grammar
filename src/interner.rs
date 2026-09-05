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
}

impl Default for InternerContext {
    fn default() -> Self {
        Self::new()
    }
}
