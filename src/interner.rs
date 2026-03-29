
use std::num::NonZeroU32;
use lasso::{ThreadedRodeo, Spur, Key};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Symbol(NonZeroU32);

impl Symbol {
    #[doc(hidden)]
    pub(crate) fn from_spur(spur: lasso::Spur) -> Self {
        Self(spur.into_inner())
    }
    
    pub(crate) fn new(val: NonZeroU32) -> Self {
        Self(val)
    }
}

pub struct InternerContext {
    backend: ThreadedRodeo,
}

impl InternerContext {
    pub fn new() -> Self {
        Self {
            backend: ThreadedRodeo::default(),
        }
    }

    #[doc(hidden)]
    pub(crate) fn intern_string(&self, text: &str) -> Symbol {
        let spur = self.backend.get_or_intern(text);
        Symbol::from_spur(spur)
    }

    pub fn resolve(&self, symbol: Symbol) -> &str {
        let spur = Spur::try_from_usize(symbol.0.get() as usize)
            .expect("Invalid Symbol ID");
            
        self.backend.resolve(&spur)
    }
}
