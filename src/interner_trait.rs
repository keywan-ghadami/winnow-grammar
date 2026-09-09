//! The interner a grammar declares it needs - ADR 22.

use crate::Symbol;

/// What `ident` and `intern(…)` need of an interner: text in, a `Symbol` out,
/// and a way back.
///
/// `Symbol` is deliberately the identity type rather than an associated one.
/// It keeps every rule's declared return type unchanged, and it costs an
/// implementation nothing that matters: a `Symbol` is a dense `u32` index, and
/// the interesting interners hand out exactly that. A slot table's slot number
/// *is* this id.
///
/// **What decides sharing, threading and merging is which type sits here.** A
/// thread-safe interner can be cloned into every piece of a `par_fold` and
/// needs no merge; a single-threaded one cannot be shared, so its symbols are
/// per piece and a merge has to remap them. That is one choice with three
/// consequences, not three choices - ADR 22 has the table.
#[diagnostic::on_unimplemented(
    message = "`interner {Self};` names a type that is not an interner",
    label = "not an interner",
    note = "implement `winnow_grammar::Interner` for it: `intern(&mut self, &str) -> Symbol` and `resolve(&self, Symbol) -> &str`"
)]
pub trait Interner {
    /// The symbol for `text`, interning it if it is new.
    fn intern(&mut self, text: &str) -> Symbol;

    /// The text a symbol stands for.
    ///
    /// Panics if the symbol did not come from this interner - the same
    /// contract [`InternerContext::resolve`](crate::InternerContext::resolve)
    /// has, and for the same reason: an index into another interner's table is
    /// not a question with an answer.
    fn resolve(&self, symbol: Symbol) -> &str;
}

/// The bound `interner I;` puts on a grammar's state: "this parse's state
/// provides an `I`, and `I` is an interner".
///
/// The same shape as [`StateOf`](crate::StateOf), and for the same reason: a
/// requirement composes where an identity does not, so one state can serve a
/// grammar's `state` declaration and its `interner` declaration at once, and
/// two grammars with different interners can run over one context.
#[diagnostic::on_unimplemented(
    message = "the grammar declares `interner {I}`, but this parse's state does not provide one",
    label = "no `{I}` in this state",
    note = "parse with a state of type `{I}`, or implement `InternerOf<{I}>` for the state you have"
)]
pub trait InternerOf<I: Interner> {
    /// The interner this state carries.
    fn interner(&mut self) -> &mut I;
}

impl<I: Interner> InternerOf<I> for I {
    #[inline]
    fn interner(&mut self) -> &mut I {
        self
    }
}
