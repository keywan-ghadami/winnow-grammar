//! The collection a grammar's repetitions collect into - `collect L;`.

/// What a bound repetition (`xs:x*`, `xs:x+`, `xs:x{n,m}`) collects its
/// elements into.
///
/// A grammar that declares nothing collects into [`Vec`], as it always has. A
/// grammar that declares `collect L;` collects into `L<T>`, which implements
/// this trait for the element type `T`:
///
/// ```
/// use winnow_grammar::Collect;
///
/// #[derive(Debug, Default, PartialEq)]
/// pub struct List<T>(Vec<T>);
///
/// impl<T> Collect<T> for List<T> {
///     fn empty() -> Self { List(Vec::new()) }
///     fn push(&mut self, item: T) { self.0.push(item) }
///     fn len(&self) -> usize { self.0.len() }
/// }
///
/// let mut l = <List<u8> as Collect<u8>>::empty();
/// Collect::push(&mut l, 7);
/// assert_eq!(Collect::len(&l), 1);
/// assert_eq!(l, List(vec![7]));
/// ```
///
/// The repetition's loop calls these three methods and nothing else; `len`
/// is how it counts against the bounds and numbers an element in an error
/// (`in item 3`). The calls are static, so a collection whose methods
/// inline costs what a hand-written loop over it would.
pub trait Collect<T> {
    /// A collection with no elements, the one a repetition starts from.
    fn empty() -> Self;
    /// Appends `item` at the end.
    fn push(&mut self, item: T);
    /// How many elements have been pushed.
    fn len(&self) -> usize;
    /// Whether nothing has been pushed yet.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Collect<T> for ::std::vec::Vec<T> {
    #[inline(always)]
    fn empty() -> Self {
        ::std::vec::Vec::new()
    }
    #[inline(always)]
    fn push(&mut self, item: T) {
        ::std::vec::Vec::push(self, item)
    }
    #[inline(always)]
    fn len(&self) -> usize {
        ::std::vec::Vec::len(self)
    }
}

impl<T> Collect<T> for ::std::collections::VecDeque<T> {
    #[inline(always)]
    fn empty() -> Self {
        ::std::collections::VecDeque::new()
    }
    #[inline(always)]
    fn push(&mut self, item: T) {
        self.push_back(item)
    }
    #[inline(always)]
    fn len(&self) -> usize {
        ::std::collections::VecDeque::len(self)
    }
}
