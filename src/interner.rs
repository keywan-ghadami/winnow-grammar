use std::num::NonZeroU32;
use std::sync::Arc;
use lasso::{ThreadedRodeo, Spur, Key};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Symbol(NonZeroU32);

impl Symbol {
    #[doc(hidden)]
    pub fn from_spur(spur: lasso::Spur) -> Self {
        Self(spur.into_inner())
    }
    
    #[doc(hidden)]
    pub fn into_spur(self) -> Spur {
        Spur::try_from_usize(self.0.get() as usize).expect("Invalid Symbol ID")
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