//! The state a grammar declares it needs - ADR 20.

/// The bound `state T;` puts on a grammar's state type: "this parse's state
/// provides a `T`".
///
/// It is a *requirement*, not an identity, and that is the whole point. A
/// grammar that declared its state by substitution would be incompatible with
/// every grammar that declared a different one; a grammar that declares a
/// requirement composes, because one state can satisfy several:
///
/// ```
/// use winnow_grammar::StateOf;
///
/// #[derive(Default)]
/// struct Symbols { count: usize }
/// #[derive(Default)]
/// struct Slots { used: usize }
///
/// // One state for two grammars, each of which declared what it needs.
/// #[derive(Default)]
/// struct App { symbols: Symbols, slots: Slots }
/// impl StateOf<Symbols> for App {
///     fn state(&mut self) -> &mut Symbols { &mut self.symbols }
/// }
/// impl StateOf<Slots> for App {
///     fn state(&mut self) -> &mut Slots { &mut self.slots }
/// }
///
/// let mut app = App::default();
/// StateOf::<Symbols>::state(&mut app).count += 1;
/// StateOf::<Slots>::state(&mut app).used += 1;
/// assert_eq!((app.symbols.count, app.slots.used), (1, 1));
/// ```
///
/// The blanket impl below means a state that simply *is* the declared type
/// satisfies the bound with nothing to write - which is the common case, and
/// the one a grammar starts from.
#[diagnostic::on_unimplemented(
    message = "the grammar declares `state {T}`, but this parse's state does not provide one",
    label = "no `{T}` in this state",
    note = "parse with a state of type `{T}`, or implement `StateOf<{T}>` for the state you have"
)]
pub trait StateOf<T> {
    /// The part of this state the grammar declared.
    fn state(&mut self) -> &mut T;
}

impl<T> StateOf<T> for T {
    #[inline]
    fn state(&mut self) -> &mut T {
        self
    }
}
