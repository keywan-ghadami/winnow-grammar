//! A `#[stream]` rule's input read from a [`Read`] - ADR 27 §3.
//!
//! **What a parse of a block may not do is depend on where the block ends.**
//! A block of the input is a prefix of it; a parser that stopped, or failed,
//! because the block ended there may have stopped elsewhere with more input.
//! In a grammar with a `#[stream]` rule every parser the code generator
//! writes is wrapped in [`edge`], which sets
//! [`ParseContext::touched`](crate::ParseContext::touched) when the parser
//! ended within `k` bytes of the end of what it was given - `k` being the
//! furthest any of the grammar's parsers looks past where it stops, or past
//! where it began when it fails. A result whose parse did not touch the end is
//! the result the whole input would have given; one that did is thrown away,
//! and the step is parsed again over more input, or - when the window already
//! holds `max_frame` bytes - reported as looking further ahead than the
//! bound on memory allows.
//!
//! **The driver runs the grammar in steps.** The generated
//! `parse_<rule>_reader` cuts the path from the stream rule to its `fold`
//! into a prefix per rule, one step per item of the fold, and a suffix per
//! rule; each step is a [`Stream::chunk`] or one of [`Stream::items`]. Between two
//! steps no view into the input is alive, so the input already parsed is
//! dropped and the next block read. Positions are moved to the stream's own:
//! errors, what the diagnosing pass recorded, spans (`ParseContext::origin`).

use super::{reader, Input, InputOf, Slice, TRIVIA_UNSEEN};
use crate::error::{Diagnostics, ParseError};
use crate::{Diagnose, ParseContext};
use std::io::Read;
use std::marker::PhantomData;
use winnow::error::{EmptyError, ErrMode};
use winnow::stream::{AsBStr, Location, Stream as _};
use winnow::Parser;

/// Runs `p` and notes in the context whether it may have depended on the end
/// of its input: it succeeded within `k` bytes of the end, or failed where it
/// began within `k` bytes of it. Only a grammar with a `#[stream]` rule is
/// generated with it.
#[inline(always)]
pub fn edge<B: Slice, Cx: crate::Ctx, O, P, E>(
    k: usize,
    mut p: P,
) -> impl FnMut(&mut InputOf<B, Cx>) -> Result<O, ErrMode<E>>
where
    P: Parser<InputOf<B, Cx>, O, ErrMode<E>>,
{
    // Only the tracked copy calls this: inlining it costs the plain
    // entries nothing, and leaves `p` to the rule body around it.
    #[inline(always)]
    move |input| {
        let before = input.eof_offset();
        let r = p.parse_next(input);
        let left = match r {
            Ok(_) => input.eof_offset(),
            Err(_) => before,
        };
        if left < k {
            input.state.cx_mut().touched = true;
        }
        r
    }
}

/// [`edge`] that also notes in the context how far a failure got
/// (`ParseContext::reach`): for a grammar whose items may record a failure
/// past their own end, which the stream's diagnosing pass then has to see.
#[inline(always)]
pub fn edge_reach<B: Slice, Cx: crate::Ctx, O, P, E>(
    k: usize,
    mut p: P,
) -> impl FnMut(&mut InputOf<B, Cx>) -> Result<O, ErrMode<E>>
where
    P: Parser<InputOf<B, Cx>, O, ErrMode<E>>,
{
    // Only the tracked copy calls this: inlining it costs the plain
    // entries nothing, and leaves `p` to the rule body around it.
    #[inline(always)]
    move |input| {
        let before = input.eof_offset();
        let r = p.parse_next(input);
        let left = match r {
            Ok(_) => input.eof_offset(),
            Err(_) => {
                let at = before.min(input.eof_offset());
                if at < input.state.cx_mut().reach {
                    input.state.cx_mut().reach = at;
                }
                before
            }
        };
        if left < k {
            input.state.cx_mut().touched = true;
        }
        r
    }
}

/// [`edge`] for a quoted literal - `string`, whose scan for its closing
/// quote has no bound: a failure is the end's doing when no closing `quote`
/// follows the opening one in what the parser was given.
#[inline(always)]
pub fn edge_quoted<B: Slice, Cx: crate::Ctx, O, P, E>(
    k: usize,
    quote: u8,
    mut p: P,
) -> impl FnMut(&mut InputOf<B, Cx>) -> Result<O, ErrMode<E>>
where
    P: Parser<InputOf<B, Cx>, O, ErrMode<E>>,
{
    let mut p = edge(k, move |i: &mut InputOf<B, Cx>| p.parse_next(i));
    move |input| {
        let cp = input.checkpoint();
        let r = p.parse_next(input);
        if r.is_err() {
            input.reset(&cp);
            let b = input.as_bstr();
            if b.first() == Some(&quote) && !closes(&b[1..], quote) {
                input.state.cx_mut().touched = true;
            }
        }
        r
    }
}

/// Is there a `quote` that no backslash stands before? Conservative: a quote
/// after an escaped backslash counts as escaped, which only makes a failure
/// look more like the end's doing than it is.
fn closes(b: &[u8], quote: u8) -> bool {
    b.iter()
        .enumerate()
        .any(|(i, &c)| c == quote && (i == 0 || b[i - 1] != b'\\'))
}

/// For a hand-written parser in a grammar with a `#[stream]` rule that looks
/// further past where it stops than the grammar's own parsers do: says that
/// this parse depended on the end of its input.
///
/// The generated code already notes a parser that stopped, or failed where
/// it began, within `k` bytes of the end - `k` being the grammar's longest
/// literal, or up to 8 bytes for its built-ins. A parser that decides further
/// away - a search for a closing delimiter, a scan to the next keyword - calls
/// this when it reached the end of its input without deciding. Without it the
/// reader takes a result that more input would have changed, and answers what
/// the parse in memory does not.
///
/// ```rust,ignore
/// match body.find("*/") {
///     Some(n) => Ok(i.next_slice(2 + n + 2)),
///     None => {
///         rt::touch(i); // searched to the end: more input might close it
///         Err(ParseError::from_stream(&i.input).add_expected("a closed comment"))
///     }
/// }
/// ```
///
/// Outside a stream it changes nothing. See *Hand-written parsers in a stream
/// grammar* in `SYNTAX.md`, and `tests/stream_extern_test.rs`.
pub fn touch<B: Slice, Cx: crate::Ctx>(input: &mut InputOf<B, Cx>) {
    input.state.cx_mut().touched = true;
}

/// A span in the input of a step, moved to the whole input's positions.
///
/// A reader parses a window of the stream, so a position a hand-written
/// parser takes itself (`current_token_start()`) counts from the window's
/// start; this moves it to the stream's. Outside a stream it is the span
/// unchanged. Spans bound with `@` are moved already.
#[inline(always)]
pub fn span_at<B: Slice, Cx: crate::Ctx>(
    input: &InputOf<B, Cx>,
    span: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let o = input.state.cx().origin;
    span.start + o..span.end + o
}

/// What a stream reads: text, checked as UTF-8 where it arrives, or bytes.
pub trait Kind {
    /// The input of one step.
    type View<'w>: Slice;
    #[doc(hidden)]
    const TEXT: bool;
    #[doc(hidden)]
    fn view(b: &[u8]) -> Self::View<'_>;
}

/// A text grammar's stream.
pub struct Text;
/// A byte grammar's stream (`input bytes;`).
pub struct Bytes;

impl Kind for Text {
    type View<'w> = &'w str;
    const TEXT: bool = true;
    #[inline]
    fn view(b: &[u8]) -> &str {
        // SAFETY: only called on the part of the buffer the stream has
        // checked as UTF-8, which ends at a character boundary (`Stream::
        // validate`).
        unsafe { std::str::from_utf8_unchecked(b) }
    }
}

impl Kind for Bytes {
    type View<'w> = &'w [u8];
    const TEXT: bool = false;
    #[inline]
    fn view(b: &[u8]) -> &[u8] {
        b
    }
}

/// Which pass a step runs in, and whether its input is the rest of the
/// stream.
#[derive(Debug, Clone, Copy)]
pub struct Pass {
    diagnose: bool,
    last: bool,
    /// A taken item parsed again, for what it records: not taken again.
    replay: bool,
}

impl Pass {
    /// The diagnosing pass, with `ParseError`; otherwise the fast one.
    pub fn diagnose(&self) -> bool {
        self.diagnose
    }

    /// Whether the step has the rest of the stream: then where its window
    /// ends cannot be what a parse depended on, and the generated reader
    /// runs the plain parsers.
    pub fn last(&self) -> bool {
        self.last
    }
}

/// How a step failed, with the error of the diagnosing pass.
#[derive(Debug)]
pub enum Fail {
    Backtrack(Option<ParseError>),
    Cut(Option<ParseError>),
    /// Not a failure: the caller of `parse_<rule>_reader_each` stopped at
    /// the item just taken. Never diagnosed, retried or replayed.
    Stop,
}

impl Fail {
    fn error(self) -> Option<ParseError> {
        match self {
            Fail::Backtrack(e) | Fail::Cut(e) => e,
            Fail::Stop => None,
        }
    }
}

/// The fast pass's error in a stream's steps: `EmptyError` under another
/// name. It carries nothing and does nothing that `EmptyError` does not; it
/// exists so that the tracked copy of a grammar with a `#[stream]` rule
/// instantiates every generic parser it reaches for a type of its own (ADR
/// 27 §3, *Per-mode copies*). With one type for both copies, each runtime
/// parser - a number, a scan, a literal - had a caller in each copy, and the
/// compiler called it from both instead of taking it into the rule, as it
/// does where there is one copy. With its own type each copy compiles as
/// the only one.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeEmpty;

impl<I: winnow::stream::Stream> winnow::error::ParserError<I> for EdgeEmpty {
    type Inner = Self;

    #[inline(always)]
    fn from_input(_: &I) -> Self {
        Self
    }

    #[inline(always)]
    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl<I: winnow::stream::Stream, C> winnow::error::AddContext<I, C> for EdgeEmpty {}

impl<I, X> winnow::error::FromExternalError<I, X> for EdgeEmpty {
    #[inline(always)]
    fn from_external_error(_input: &I, _e: X) -> Self {
        Self
    }
}

impl std::fmt::Display for EdgeEmpty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        "failed to parse".fmt(f)
    }
}

impl crate::Diagnostics for EdgeEmpty {
    const RECORDING: bool = false;

    #[inline(always)]
    fn fail<I: winnow::stream::Stream + winnow::stream::Location + winnow::stream::AsBStr>(
        _input: &I,
        _message: &'static str,
    ) -> Self {
        Self
    }

    #[inline(always)]
    fn item(self, _index: usize) -> Self {
        self
    }

    #[inline(always)]
    fn labelled(self, _start: usize, _label: &'static str) -> Self {
        self
    }

    #[inline(always)]
    fn expected(self, _start: usize, _what: &'static str) -> Self {
        self
    }

    #[inline(always)]
    fn in_lookahead(self) -> Self {
        self
    }

    #[inline(always)]
    fn record<S>(&self, _ctx: &mut crate::ParseContext<S>, _start: usize) {}

    #[inline(always)]
    fn record_if_begun<S>(&self, _ctx: &mut crate::ParseContext<S>, _start: usize) {}

    #[inline(always)]
    fn from_parse_error(_e: ParseError) -> Self {
        Self
    }

    #[inline(always)]
    fn into_parse_error(self) -> Option<ParseError> {
        None
    }

    #[inline(always)]
    fn external<
        I: winnow::stream::Stream + winnow::stream::Location + winnow::stream::AsBStr,
        X: std::fmt::Display,
    >(
        _input: &I,
        _e: X,
    ) -> Self {
        Self
    }
}

impl From<ErrMode<EdgeEmpty>> for Fail {
    fn from(e: ErrMode<EdgeEmpty>) -> Self {
        match e {
            ErrMode::Backtrack(_) => Fail::Backtrack(None),
            _ => Fail::Cut(None),
        }
    }
}

impl From<ErrMode<EmptyError>> for Fail {
    fn from(e: ErrMode<EmptyError>) -> Self {
        match e {
            ErrMode::Backtrack(_) => Fail::Backtrack(None),
            _ => Fail::Cut(None),
        }
    }
}

impl From<ErrMode<ParseError>> for Fail {
    fn from(e: ErrMode<ParseError>) -> Self {
        match e {
            ErrMode::Backtrack(e) => Fail::Backtrack(Some(e)),
            ErrMode::Cut(e) => Fail::Cut(Some(e)),
            ErrMode::Incomplete(_) => Fail::Cut(None),
        }
    }
}

/// Whether an item just parsed is taken: it consumed something, and did not
/// depend on the end of its input - unless that end is the stream's. The
/// generated item step calls the fold's step only when this holds.
#[inline]
pub fn commit<B: Slice, Cx: crate::Ctx>(input: &InputOf<B, Cx>, pass: &Pass) -> bool {
    !pass.replay && input.current_token_start() > 0 && (pass.last || !input.state.cx().touched)
}

/// The input of a step that the stream runs once - a prefix, a suffix, the
/// end, a replay: the window, with the stream's context lent to it rather
/// than moved in and back (two copies of it a step, most of what a short
/// stream cost).
pub type Step<'w, 'x, K, S> = InputOf<<K as Kind>::View<'w>, &'x mut ParseContext<S>>;

/// The input of the item loop: the window, holding the context for the
/// loop's length, moved in and back once a window. Its fields are at fixed
/// places in the loop's frame; lent, LLVM loads the pointer to them again
/// after every item and keeps nothing across it in registers
/// (`stream_profile stream`: 26 instructions an item).
pub type Held<'w, K, S> = Input<<K as Kind>::View<'w>, S>;

/// The stream's context moved out of the caller's for the item loop, which
/// holds it ([`Held`]): read out once a window and written back - when the
/// loop is done, or as a panic unwinds through it, so that the caller's
/// context is never left moved-from.
struct Hold<'h, V, S> {
    home: &'h mut ParseContext<S>,
    input: std::mem::ManuallyDrop<Input<V, S>>,
}

impl<'h, V: Clone + winnow::stream::Offset, S> Hold<'h, V, S> {
    #[inline(always)]
    fn new(home: &'h mut ParseContext<S>, view: V) -> Self {
        // SAFETY: the context is moved out of `home`, which `Hold` borrows
        // until it writes the context back (`put_back`, or `drop` as a
        // panic unwinds): nothing reads or drops `home` in between, and the
        // copy left in it is overwritten, not dropped.
        let state = unsafe { std::ptr::read(&*home) };
        Hold {
            home,
            input: std::mem::ManuallyDrop::new(Input {
                input: winnow::stream::LocatingSlice::new(view),
                state,
            }),
        }
    }

    /// Writes the context back and gives back where it came from.
    #[inline(always)]
    fn put_back(self) -> &'h mut ParseContext<S> {
        let mut this = std::mem::ManuallyDrop::new(self);
        // SAFETY: `this` is not dropped, so `input` is taken once, here; the
        // borrow of `home` is moved out of it once, here.
        unsafe {
            let input = std::mem::ManuallyDrop::take(&mut this.input);
            let home: &'h mut ParseContext<S> = std::ptr::read(&this.home);
            std::ptr::write(&mut *home, input.state);
            home
        }
    }
}

impl<V, S> Drop for Hold<'_, V, S> {
    fn drop(&mut self) {
        // SAFETY: only as a panic unwinds (`put_back` forgets `Hold`): the
        // context goes back over the bits it was read from.
        unsafe {
            let input = std::mem::ManuallyDrop::take(&mut self.input);
            std::ptr::write(&mut *self.home, input.state);
        }
    }
}

/// What the item loop of a window gives back: how it stopped, where, the
/// context, the last item taken and the failures that reach past their item.
type Scanned<'h, S> = (
    Stop,
    usize,
    &'h mut ParseContext<S>,
    Option<usize>,
    Vec<Reaching>,
    Option<usize>,
);

/// Why a step stopped parsing items from its window.
enum Stop {
    /// An item may need more input than the window has: read more.
    Refill,
    /// An item needs more than the lookahead holds.
    TooFar,
    /// The items ended.
    Ended,
    /// An item was taken and the caller wants no more.
    Halted,
    /// An item failed.
    Failed(Fail),
}

/// An item taken whose failures reached its own end: where it starts, the
/// furthest a failure of it got, and its number - all in `buf`.
#[derive(Debug, Clone, Copy)]
struct Reaching {
    start: usize,
    reach: usize,
    n: usize,
}

/// A step's parser as the diagnosing pass runs it again, owning what it
/// needs: the generated reader hands one over (`keep`) only when the stream
/// asks, for a step whose records are left for later (ADR 17: explain on
/// failure).
///
/// A trait object of its own rather than a boxed `dyn for<'w> FnMut(&mut
/// Input<K::View<'w>, S>, ..)`: that projection inside the object type made
/// the nightly compiler under Miri fail in borrowck ("unnormalized
/// aliases").
pub type Replayer<'c, K, S> = Box<dyn Rerun<K, S> + 'c>;

/// What a [`Replayer`] does: parse the step once more, its value thrown
/// away.
#[doc(hidden)]
pub trait Rerun<K: Kind, S> {
    fn rerun(&mut self, input: &mut Step<'_, '_, K, S>, pass: &Pass, n: usize) -> Result<(), Fail>;
}

/// A step's parser as a [`Rerun`]; `T` is its value's type.
struct Keep<F, T>(F, PhantomData<fn() -> T>);

impl<K: Kind, S, T, F> Rerun<K, S> for Keep<F, T>
where
    F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail>,
{
    fn rerun(&mut self, input: &mut Step<'_, '_, K, S>, pass: &Pass, n: usize) -> Result<(), Fail> {
        (self.0)(input, pass, n).map(|_| ())
    }
}

/// A [`Replayer`] of `f`.
#[doc(hidden)]
pub fn replayer<'c, K: Kind + 'c, S: Clone + std::fmt::Debug + 'c, T: 'c, F>(
    f: F,
) -> Replayer<'c, K, S>
where
    F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail> + 'c,
{
    Box::new(Keep(f, PhantomData))
}

/// What a matched step would have recorded, not yet recorded. The parse in
/// memory records it only on its way to an error, once its fast pass has
/// failed; so the stream parses the step again for it only once a failure
/// comes (or a cut sets the records aside), and forgets it once the stream
/// is past where its failures reached.
struct Pending<'c, K: Kind, S> {
    run: Replayer<'c, K, S>,
    rules: &'static [&'static str],
    /// The user state the replay starts from.
    state: S,
    what: Replay,
    /// The furthest its failures may have got, in `buf`.
    reach: usize,
}

/// Which replay a [`Pending`] is.
enum Replay {
    /// A prefix or suffix, at its start in `buf` ([`Stream::record_chunk`]).
    Chunk(usize),
    /// The end of a fold (`Stop::Ended` in [`Stream::settle`]): the items
    /// taken before it whose failures may reach it, oldest first, the last
    /// item taken, and the item that did not come - each at its start in
    /// `buf`, with its number.
    Ended(Ended),
}

/// The items a [`Replay::Ended`] parses again, oldest first. The last item
/// taken and the one that did not come are held inline: a fold that ends
/// cleanly has no earlier ones, and a Vec for its one or two items was an
/// allocation a stream (`stream_profile list`).
struct Ended {
    earlier: Vec<(usize, usize)>,
    prev: Option<(usize, usize)>,
    next: (usize, usize),
}

impl Ended {
    fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.earlier
            .iter()
            .copied()
            .chain(self.prev)
            .chain(std::iter::once(self.next))
    }

    fn shift_back(&mut self, by: usize) {
        for i in self
            .earlier
            .iter_mut()
            .chain(self.prev.as_mut())
            .chain(std::iter::once(&mut self.next))
        {
            i.0 -= by;
        }
    }
}

impl<K: Kind, S> Pending<'_, K, S> {
    /// The first byte of `buf` it parses again.
    fn start(&self) -> usize {
        match &self.what {
            Replay::Chunk(s) => *s,
            Replay::Ended(items) => items.iter().map(|i| i.0).min().unwrap_or(self.reach),
        }
    }

    fn shift_back(&mut self, by: usize) {
        self.reach -= by;
        match &mut self.what {
            Replay::Chunk(s) => *s -= by,
            Replay::Ended(items) => items.shift_back(by),
        }
    }
}

/// The passes of a `parse_<rule>_slice`, as [`super::entry`] runs the
/// parse in memory (ADR 17): a stream with nothing diagnosed (`Diagnose::Off`)
/// and, only when that fails, one more with every step diagnosed
/// (`Diagnose::Eager`) - under `Replay` on the user state from before the
/// first, under `ReplayInPlace` on the state the first left. A parse that
/// succeeds costs the fast stream alone: no step is kept for a diagnosis
/// that does not come, and no cut has records to set aside.
///
/// `run` is one stream over the input; its flag says it is the second
/// pass, whose items `each` has already seen.
#[doc(hidden)]
pub fn slice_passes<S: Clone + std::fmt::Debug, T>(
    context: &mut ParseContext<S>,
    mut run: impl FnMut(&mut ParseContext<S>, bool) -> Result<T, ParseError>,
) -> Result<T, ParseError> {
    let mode = context.diagnose;
    if mode == Diagnose::Eager {
        return run(context, false);
    }
    let snapshot = (mode == Diagnose::Replay).then(|| context.user_state.clone());
    context.diagnose = Diagnose::Off;
    let fast = run(context, false);
    context.diagnose = mode;
    match fast {
        Ok(v) => return Ok(v),
        Err(e) if mode == Diagnose::Off => return Err(e),
        Err(_) => {}
    }
    if let Some(s) = snapshot {
        context.user_state = s;
    }
    context.diagnose = Diagnose::Eager;
    let r = run(context, true);
    context.diagnose = mode;
    r
}

/// Shows `each` an item, unless `again`: the second pass of a
/// `parse_<rule>_slice_each` ([`slice_passes`]), whose items it has seen.
#[doc(hidden)]
#[inline(always)]
pub fn see<T: ?Sized, B>(
    again: bool,
    each: &mut impl FnMut(&T) -> std::ops::ControlFlow<B>,
    item: &T,
) -> std::ops::ControlFlow<B> {
    if again {
        std::ops::ControlFlow::Continue(())
    } else {
        each(item)
    }
}

/// What the steps read: the input all in memory under `WHOLE`, the read
/// buffer otherwise - decided when the stream is compiled, so the reader
/// pays no test for it. A macro, so that it borrows those two fields only.
macro_rules! bytes {
    ($s:expr) => {
        if WHOLE {
            $s.src
        } else {
            $s.buf.bytes()
        }
    };
}

/// The length of [`bytes!`].
macro_rules! filled {
    ($s:expr) => {
        if WHOLE {
            $s.src.len()
        } else {
            $s.buf.len()
        }
    };
}

/// The reader, the buffer and the context of a stream parse. See the module
/// documentation; the generated `parse_<rule>_reader` is its only user.
pub struct Stream<
    'c,
    K: Kind,
    R: Read,
    S: Clone + std::fmt::Debug,
    const TRACK: bool = false,
    const WHOLE: bool = false,
> {
    reader: R,
    block: usize,
    /// How far past the start of a step a parse may look before the step is
    /// an error: `max_frame` plus the grammar's own lookahead.
    lookahead: usize,
    max_frame: usize,
    buf: reader::Buf,
    /// The start of the next step, in `buf`.
    pos: usize,
    /// The start of the last item taken, in `buf`: kept, so that a failure
    /// after it can be diagnosed as the parse in memory would diagnose it.
    prev_item: Option<usize>,
    /// Items taken before it whose failures reached their own end: an
    /// alternative they tried and abandoned, or a lookahead, looked at what
    /// follows them. The parse in memory records those on its way to an
    /// error further on, so the diagnosing pass replays them too while what
    /// they reached is not behind the next step. In `buf`, oldest first.
    reaching: Vec<Reaching>,
    /// Matched steps whose records the diagnosing pass has not taken yet,
    /// oldest first: see [`Pending`].
    pending: Vec<Pending<'c, K, S>>,
    /// The input all in memory, under `WHOLE` ([`Stream::whole`]): the
    /// steps read it, not `buf`, and nothing is copied. Empty otherwise.
    src: &'c [u8],
    /// The grammar's own lookahead.
    k: usize,
    /// The stream's offset of `buf[0]`.
    base: usize,
    /// The end of what a step may see: for text, of what is checked as UTF-8.
    visible: usize,
    eof: bool,
    /// The caller's context, which the parse runs on: taken out of here for
    /// a step and put back after it - a pointer moved, not the context.
    ctx: Option<&'c mut ParseContext<S>>,
    /// What the diagnosing pass recorded, at stream positions.
    furthest: Option<ParseError>,
    /// What a cut set aside that is still open after the step that made
    /// it, at stream positions: the stream's side of
    /// `ParseContext::cut_stash`. See [`Stream::cut_end`].
    ///
    /// No error of a step is merged with it, backtracking or not. While it
    /// holds an entry a cut scope spanning steps is open, and in memory
    /// every failure under that scope leaves it as a cut - the call after
    /// the cut is `cut_err`, the trivia skipped there `skip_trivia_cut` - so
    /// `best_after_cut` would ignore it there too.
    /// A step reports such a failure as a `Backtrack` when the rule it
    /// parses has no cut of its own; the stream does not tell them apart.
    cut_stash: Vec<Option<ParseError>>,
    _kind: PhantomData<K>,
}

impl<'c, K: Kind, R: Read, S: Clone + std::fmt::Debug, const TRACK: bool, const WHOLE: bool>
    Stream<'c, K, R, S, TRACK, WHOLE>
{
    /// A stream over `reader`, parsing on `context`: user state, recoveries
    /// and all are there as the parse leaves them - no copy is made. `k` is
    /// the grammar's own lookahead; `TRACK`, whether its parsers note how far
    /// failures reach (`edge_reach`), so that the items an error further on
    /// has to be diagnosed with are kept.
    pub fn new(
        reader: R,
        context: &'c mut ParseContext<S>,
        reading: super::Reading,
        k: usize,
    ) -> Self {
        context.begin_parse();
        Stream {
            reader,
            block: reading.block.max(1),
            lookahead: reading.max_frame.saturating_add(k),
            max_frame: reading.max_frame,
            buf: reader::Buf::default(),
            pos: 0,
            prev_item: None,
            reaching: Vec::new(),
            pending: Vec::new(),
            src: &[],
            k,
            base: 0,
            visible: 0,
            eof: false,
            ctx: Some(context),
            furthest: None,
            cut_stash: Vec::new(),
            _kind: PhantomData,
        }
    }

    /// The context, for the actions the driver runs between steps.
    pub fn context(&mut self) -> &mut ParseContext<S> {
        self.ctx
            .as_deref_mut()
            .expect("the context is back between steps")
    }

    /// Where a cut scope that spans steps begins: a rule of the path with a
    /// cut `=>` before the element the path continues in. The generated
    /// reader takes it before the rule's prefix and hands it to
    /// [`Stream::cut_end`] after the rule's suffix.
    #[doc(hidden)]
    pub fn cut_depth(&self) -> usize {
        self.cut_stash.len()
    }

    /// The end of a cut scope that spans steps - the rule matched: what its
    /// cut set aside competes again, as `rt::cut_end` has it in memory.
    #[doc(hidden)]
    pub fn cut_end(&mut self, depth: usize) {
        if self.cut_stash.len() <= depth {
            return;
        }
        // What the cut set aside competes with everything recorded since.
        self.flush();
        let mut back = self.furthest.take();
        for f in self.cut_stash.drain(depth..).flatten() {
            back = Some(match back {
                Some(b) => f.merge(b),
                None => f,
            });
        }
        self.furthest = back;
    }

    /// One step: `f` parses a prefix or a suffix, whose value does not
    /// borrow the input. `rules` are the rules of the path the step is
    /// inside, outermost first.
    ///
    /// `keep` makes the step's [`Replayer`]: the stream asks for it when the
    /// step matched, for what its diagnosing pass would record should a
    /// failure follow.
    pub fn chunk<T, F, G>(
        &mut self,
        rules: &'static [&'static str],
        mut f: F,
        keep: G,
    ) -> Result<T, ParseError>
    where
        F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let r = self.run(rules, &mut f, keep);
        self.finished(r)
    }

    /// Items of the fold, as many as the input that has arrived decides: `f`
    /// parses one and, where [`commit`] says so, hands it to the step,
    /// answering whether it did. Its third argument is the number of the
    /// item, from 1: a separator comes before every item but the first, and
    /// `max = N` ends the fold before item N + 1.
    ///
    /// The number is an argument and not a field of [`Pass`]: a `Pass` that
    /// changes from item to item keeps LLVM from hoisting the pass out of
    /// the loop, which cost every streamed fold 31 instructions per item
    /// (callgrind, `examples/stream_profile.rs`), whether it used the number
    /// or not. A fold without `sep` and `max` leaves the argument unread. `n` counts the items taken, and numbers an
    /// item in an error. `true` asks to be called again, after a read;
    /// `false` is the end of the fold, at the position before the item that
    /// did not come.
    ///
    /// One window serves every item it decides, so a step costs the item's
    /// parse and little else; an item that touched the end of the window is
    /// parsed again in the next.
    ///
    /// `keep` makes the item's [`Replayer`], as for [`Stream::chunk`]: asked
    /// for when the fold ends.
    pub fn items<F, G>(
        &mut self,
        rules: &'static [&'static str],
        n: &mut usize,
        mut f: F,
        keep: G,
    ) -> Result<bool, ParseError>
    where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<bool, Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let r = self.items_once(rules, n, &mut f, keep);
        // An item hands its emits to `each` when it is taken, inside `f`:
        // what is left is an attempt's that was not (ADR 28).
        self.context().emits.discard();
        self.finished(r)
    }

    fn items_once<F, G>(
        &mut self,
        rules: &'static [&'static str],
        n: &mut usize,
        f: &mut F,
        keep: G,
    ) -> Result<bool, ParseError>
    where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<bool, Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let mode = self.context().diagnose;
        let zst = std::mem::size_of::<S>() == 0;
        let lazy = zst || mode == Diagnose::Replay;
        if lazy {
            if self.visible == self.pos && !self.eof {
                self.read_once(0)?;
            }
        } else {
            while self.visible - self.pos < self.lookahead && !self.eof {
                self.read_once(self.lookahead - (self.visible - self.pos))?;
            }
        }
        let last = self.eof && self.visible == filled!(self);
        let pass = Pass {
            diagnose: mode == Diagnose::Eager,
            last,
            replay: false,
        };
        let origin = self.base + self.pos;
        let ctx = self.ctx.take().expect("the context is back between steps");
        let recovered = ctx.recovered.len();
        ctx.touched = false;
        ctx.trivia_memo = TRIVIA_UNSEEN;
        ctx.last_trivia = (usize::MAX, usize::MAX);
        ctx.furthest = None;
        // `end_step` emptied it when the last step ended.
        debug_assert!(ctx.cut_stash.is_empty());
        ctx.origin = origin;
        ctx.rules.clear();
        if pass.diagnose {
            ctx.rules.extend_from_slice(rules);
        }
        let (stop, at, mut ctx, prev, reaching, reach) = if pass.last {
            self.scan_window::<true, F>(ctx, f, n, &pass, mode, lazy)
        } else {
            self.scan_window::<false, F>(ctx, f, n, &pass, mode, lazy)
        };
        ctx.rules.clear();
        ctx.origin = 0;
        // Checked here: the flush is cold, and nearly every step has nothing
        // pending.
        if !ctx.cut_stash.is_empty() && !self.pending.is_empty() {
            self.ctx = Some(ctx);
            self.flush_before_cut();
            ctx = self.ctx.take().expect("the flush gives the context back");
        }
        end_step(&mut self.furthest, &mut self.cut_stash, ctx, origin);
        for e in &mut ctx.recovered[recovered..] {
            *e = shift(e.clone(), origin);
        }
        self.ctx = Some(ctx);
        if let Some(p) = prev {
            self.prev_item = Some(self.pos + p);
        }
        let pos = self.pos;
        let reach = reach.map(|r| pos + r);
        self.reaching.extend(reaching.into_iter().map(|r| Reaching {
            start: pos + r.start,
            reach: pos + r.reach,
            n: r.n,
        }));
        self.pos += at;
        self.prune();
        self.settle(stop, rules, n, f, &pass, mode, keep, reach)
    }

    /// Parses items from the window until one stops the step: the item
    /// loop, whole, with where it stopped, the context back, the start of
    /// the last item taken and the failures that reach past their item.
    fn scan_window<'h, const LAST: bool, F>(
        &self,
        home: &'h mut ParseContext<S>,
        f: &mut F,
        n: &mut usize,
        pass: &Pass,
        mode: Diagnose,
        lazy: bool,
    ) -> Scanned<'h, S>
    where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<bool, Fail>,
    {
        let zst = std::mem::size_of::<S>() == 0;
        debug_assert_eq!(LAST, pass.last);
        let last = LAST;
        let window = self.visible - self.pos;
        let lookahead = self.lookahead;
        let mut prev = None;
        let (k, track) = (self.k, TRACK);
        let mut reaching = Vec::new();
        let mut hold = Hold::new(home, K::view(&bytes!(self)[self.pos..self.visible]));
        let input: &mut Held<'_, K, S> = &mut hold.input;
        // What an eager pass recorded before the item being parsed.
        let mut recorded = None;
        let stop = loop {
            let at = input.current_token_start();
            // Filled to the lookahead, every item gets all of it: one that
            // would start with less waits for the next window.
            if !lazy && !last && window - at < lookahead {
                break Stop::Refill;
            }
            let snapshot = (!zst && (mode == Diagnose::Replay || (lazy && !last)))
                .then(|| input.state.user_state.clone());
            let saved = (input.state.recoveries, input.state.recovered.len());
            // What an eager pass recorded before this item: an attempt
            // that is parsed again with more input records nothing.
            if pass.diagnose {
                recorded = saved_furthest(&input.state);
            }
            input.state.touched = false;
            if track {
                input.state.reach = usize::MAX;
            }
            let cp = input.checkpoint();
            // Nothing is held here (ADR 28): an item that was taken handed
            // its emits to `each` inside `f`, and an attempt that was not
            // ends the loop, after which `items` discards what it emitted.
            // Not discarded here: this is the hot loop of every stream.
            debug_assert!(input.state.emits.is_empty());
            let r = f(input, pass, *n + 1);
            if input.state.touched && !last {
                input.reset(&cp);
                if let Some(s) = snapshot {
                    input.state.user_state = s;
                }
                input.state.recoveries = saved.0;
                input.state.recovered.truncate(saved.1);
                // An item begins with no cut open - the window starts
                // with none, and an item that matched closed its own -
                // so what this try left open goes.
                drop_open_cuts(&mut input.state.cut_stash);
                if pass.diagnose {
                    input.state.furthest = recorded.take();
                }
                break if window - at >= lookahead {
                    Stop::TooFar
                } else {
                    Stop::Refill
                };
            }
            match r {
                Ok(true) => {
                    *n += 1;
                    prev = Some(at);
                    // A failure within `k` of where the item ended, or
                    // past it: one of what follows may have to beat it.
                    let reach = input.state.reach;
                    if track && reach <= input.eof_offset() + k {
                        reaching.push(Reaching {
                            start: at,
                            reach: window - reach,
                            n: *n,
                        });
                    }
                }
                Ok(false) => break Stop::Ended,
                // Taken - `commit` held, so it did not touch the end -
                // and the caller wants no more.
                Err(Fail::Stop) => {
                    *n += 1;
                    break Stop::Halted;
                }
                Err(Fail::Backtrack(e)) => {
                    input.reset(&cp);
                    if let Some(e) = e {
                        e.item(*n + 1).record(&mut input.state, at);
                    }
                    break Stop::Ended;
                }
                Err(fail) => {
                    input.reset(&cp);
                    if let Some(s) = snapshot {
                        input.state.user_state = s;
                    }
                    break Stop::Failed(fail);
                }
            }
        };
        // How far the failures of the item that ended the fold got, in the
        // window: unknown (`None`) where the grammar does not track it.
        let reach = (track && input.state.reach != usize::MAX).then(|| window - input.state.reach);
        let at = input.current_token_start();
        (stop, at, hold.put_back(), prev, reaching, reach)
    }

    /// Acts on how the step stopped: reads more, takes the error, or parses
    /// again with the diagnosing pass for what the fast one did not record.
    #[allow(clippy::too_many_arguments)]
    fn settle<F, G>(
        &mut self,
        stop: Stop,
        rules: &'static [&'static str],
        n: &mut usize,
        f: &mut F,
        pass: &Pass,
        mode: Diagnose,
        keep: G,
        reach: Option<usize>,
    ) -> Result<bool, ParseError>
    where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<bool, Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let last = pass.last;
        let diagnose = Pass {
            diagnose: true,
            last,
            replay: false,
        };
        match stop {
            Stop::Refill => {
                let more = (self.visible - self.pos).max(self.block);
                let before = self.visible;
                while self.visible == before && !self.eof {
                    self.read_once(more)?;
                }
                Ok(true)
            }
            Stop::TooFar => Err(self.too_far()),
            Stop::Halted => Ok(false),
            Stop::Ended => {
                // What the item expected is kept for an error further on, as
                // the fold keeps it - and so is what the item before it
                // recorded, which the parse in memory would have recorded on
                // its way here. Kept to be parsed again should that error
                // come (`Pending`), not parsed again now.
                if !pass.diagnose && mode != Diagnose::Off {
                    self.defer_ended(rules, *n, keep, reach);
                }
                Ok(false)
            }
            Stop::Failed(fail) if pass.diagnose => {
                let e = fail.error().map(|mut e| {
                    for r in rules.iter().rev() {
                        e.push_rule(r);
                    }
                    let e = shift(e, self.base + self.pos);
                    match &self.furthest {
                        Some(f) => f.clone().merge(e),
                        None => e,
                    }
                });
                Err(e.unwrap_or_else(ParseError::undiagnosed))
            }
            Stop::Failed(_) if mode == Diagnose::Off => Err(ParseError::undiagnosed()),
            Stop::Failed(_) => {
                self.flush();
                let keep = self.context().user_state.clone();
                self.replay_prev(rules, f, Some(*n + 1), last);
                self.context().user_state = keep;
                match self
                    .attempt_held(self.pos, rules, f, diagnose, Some(*n + 1))
                    .0
                {
                    // The diagnosing pass took what the fast one did not.
                    Ok(_) => Ok(true),
                    Err(Fail::Stop) => Ok(false),
                    Err(fail) => Err(fail.error().unwrap_or_else(ParseError::undiagnosed)),
                }
            }
        }
    }

    /// A fold that took fewer items than its `min = N`: it fails with what
    /// its next item expected - recorded when the items ended - as the fold
    /// in memory does.
    pub fn short(&mut self) -> ParseError {
        if self.context().diagnose == Diagnose::Off {
            return ParseError::undiagnosed();
        }
        self.flush();
        let e = match &self.furthest {
            Some(f) => f.clone(),
            None => {
                let ctx = self.ctx.take().expect("the context is back between steps");
                let (e, ctx) = {
                    let input = InputOf {
                        input: winnow::stream::LocatingSlice::new(K::view(
                            &bytes!(self)[self.pos..self.visible],
                        )),
                        state: ctx,
                    };
                    (ParseError::from_stream(&input), input.state)
                };
                self.ctx = Some(ctx);
                self.absolute(e)
            }
        };
        match self.finished(Err::<(), _>(e)) {
            Err(e) => e,
            Ok(()) => unreachable!("an error stays one"),
        }
    }

    /// The end: `f` skips what the entry point skips after its rule, and then
    /// nothing may be left.
    pub fn finish<F, G>(&mut self, f: F, keep: G) -> Result<(), ParseError>
    where
        F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<(), Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        // Every cut scope is closed by now: a step closes its own, the
        // reader closes those that span steps, and a step parsed again
        // gives back what its try set aside.
        debug_assert!(
            self.cut_stash.is_empty(),
            "an open cut at the end of the stream: {:?}",
            self.cut_stash
        );
        self.chunk(&[], f, keep)?;
        while self.visible == self.pos && !self.eof {
            self.read_once(0)?;
        }
        if self.visible == self.pos && filled!(self) == self.pos {
            return Ok(());
        }
        if self.context().diagnose == Diagnose::Off {
            return Err(ParseError::undiagnosed());
        }
        self.flush();
        let ctx = self.ctx.take().expect("the context is back between steps");
        let (e, ctx) = {
            let input = InputOf {
                input: winnow::stream::LocatingSlice::new(K::view(
                    &bytes!(self)[self.pos..self.visible],
                )),
                state: ctx,
            };
            let e = ParseError::from_stream(&input).add_expected("end of input");
            (e, input.state)
        };
        self.ctx = Some(ctx);
        let e = self.absolute(e);
        let e = match &self.furthest {
            Some(f) => f.clone().merge(e),
            None => e,
        };
        self.finished(Err(e))
    }

    /// An error on its way out, with the token it found read in full: the
    /// window may have ended inside it.
    fn finished<T>(&mut self, r: Result<T, ParseError>) -> Result<T, ParseError> {
        let Err(mut e) = r else {
            return r;
        };
        if e.is_undiagnosed() || e.invalid_utf8.is_some() || e.offset < self.base {
            return Err(e);
        }
        // A read may drop what lies before the step, never what the error is
        // about: it lies after the start of the step or of the item before.
        while self.visible < e.offset - self.base + 64 && !self.eof {
            if self.read_once(0).is_err() {
                break;
            }
        }
        let at = e.offset - self.base;
        if at <= self.visible {
            e.found = crate::error::next_word(&bytes!(self)[at..self.visible]);
        }
        e.excerpt = if WHOLE {
            reader::Lines {
                bytes: self.src,
                dropped: 0,
                lines: 0,
                line_start: 0,
            }
            .excerpt(e.offset)
        } else {
            self.buf.excerpt(e.offset)
        };
        Err(e)
    }

    /// The step of [`Stream::chunk`].
    fn run<T, F, G>(
        &mut self,
        rules: &'static [&'static str],
        f: &mut F,
        keep: G,
    ) -> Result<T, ParseError>
    where
        F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail>,
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let mut keep = Some(keep);
        let mode = self.context().diagnose;
        let zst = std::mem::size_of::<S>() == 0;
        // Parse with what is there, and again with more where that was not
        // enough - unless doing it again would need a copy of a user state
        // that is not otherwise taken; then the window is filled first.
        let lazy = zst || mode == Diagnose::Replay;
        if lazy {
            if self.visible == self.pos && !self.eof {
                self.read_once(0)?;
            }
        } else {
            while self.visible - self.pos < self.lookahead && !self.eof {
                self.read_once(self.lookahead - (self.visible - self.pos))?;
            }
        }
        loop {
            let window = self.visible - self.pos;
            let mut last = self.eof && self.visible == filled!(self);
            let full = window >= self.lookahead;
            let snapshot = (!zst && (mode == Diagnose::Replay || (lazy && !last && !full)))
                .then(|| self.context().user_state.clone());
            let saved = {
                let c = self.context();
                (c.recoveries, c.recovered.len())
            };
            let first = Pass {
                diagnose: mode == Diagnose::Eager,
                last,
                replay: false,
            };
            let depth = self.cut_depth();
            // A step's emits are handed to `each` after `chunk` returns: what
            // is held then is the taken attempt's, and only that (ADR 28).
            self.context().emits.discard();
            if TRACK {
                self.context().reach = usize::MAX;
            }
            let (r, touched) = self.attempt(self.pos, rules, f, first, None);
            if touched && !last {
                if full {
                    return Err(self.too_far());
                }
                // Read first: when nothing more comes, the window was the
                // rest of the stream and the attempt is the answer - parsed
                // again, it would only be parsed over the same bytes.
                let more = window.max(self.block);
                // In stream positions: a read may drop what lies before.
                let before = self.base + self.visible;
                while self.base + self.visible == before && !self.eof {
                    self.read_once(more)?;
                }
                if self.base + self.visible != before || self.visible != filled!(self) {
                    // What the attempt's cut set aside comes back: it is
                    // parsed again, and sets it aside again if it still cuts.
                    self.cut_end(depth);
                    self.roll_back(snapshot, saved);
                    continue;
                }
                last = true;
            }
            let diagnose = Pass {
                diagnose: true,
                last,
                replay: false,
            };
            return match r {
                Ok((v, consumed)) => {
                    if !first.diagnose && mode != Diagnose::Off {
                        let reach = self.context().reach;
                        // A step that leaves a cut open has its records set
                        // aside with it: recorded now.
                        match keep.take() {
                            Some(keep) if self.cut_stash.len() == depth => {
                                self.defer_chunk(rules, keep, snapshot, consumed, reach)
                            }
                            _ => self.record_chunk(rules, f, snapshot, last),
                        }
                    }
                    self.pos += consumed;
                    Ok(v)
                }
                Err(fail) if first.diagnose => {
                    Err(fail.error().unwrap_or_else(ParseError::undiagnosed))
                }
                Err(_) if mode == Diagnose::Off => Err(ParseError::undiagnosed()),
                Err(_) => {
                    if let Some(s) = snapshot {
                        self.context().user_state = s;
                    }
                    self.flush();
                    self.context().emits.discard();
                    match self.attempt(self.pos, rules, f, diagnose, None).0 {
                        Ok((v, consumed)) => {
                            self.pos += consumed;
                            Ok(v)
                        }
                        Err(fail) => Err(fail.error().unwrap_or_else(ParseError::undiagnosed)),
                    }
                }
            };
        }
    }

    /// A prefix or suffix the fast pass matched, parsed again by the
    /// diagnosing pass for what it records, its value and effects thrown
    /// away. The parse in memory replays a failed fast pass from the start,
    /// so what a step that matched would record is there for an error
    /// further on; a stream never goes back that far. A step is parsed once
    /// per rule of the path, not once per item, so it costs that and the
    /// copy of the user state `Diagnose::Replay` takes anyway.
    ///
    /// The state goes in as the replay in memory has it: put back under
    /// `Replay` (`snapshot`), as the fast pass left it under `ReplayInPlace`.
    /// A replay that touches the end of the window records over input that
    /// is not all there: then nothing of it is kept.
    fn record_chunk<T, F>(
        &mut self,
        rules: &[&'static str],
        f: &mut F,
        snapshot: Option<S>,
        last: bool,
    ) where
        F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail>,
    {
        // What the fast pass recovered stays; the replay's goes - and so
        // does what it emitted.
        let saved = {
            let c = self.context();
            (c.recoveries, c.recovered.len())
        };
        let emitted = self.context().emits.set_aside();
        let keep = self.context().user_state.clone();
        if let Some(s) = snapshot {
            self.context().user_state = s;
        }
        let before = (self.furthest.clone(), self.cut_stash.len());
        let pass = Pass {
            diagnose: true,
            last,
            replay: true,
        };
        let (_, touched) = self.attempt(self.pos, rules, f, pass, None);
        if touched && !last {
            self.furthest = before.0;
            self.cut_stash.truncate(before.1);
        }
        self.context().user_state = keep;
        self.roll_back(None, saved);
        self.context().emits.restore(emitted);
    }

    /// A prefix or suffix the fast pass matched, kept to be parsed again
    /// as [`Stream::record_chunk`] does, should a failure come while what it
    /// records may still compete. `reach` is the context's: how far its
    /// failures got, before the end of the window - or `usize::MAX`, none
    /// noted. Without it (`TRACK` off) its failures stay within the step and
    /// its lookahead.
    fn defer_chunk<G>(
        &mut self,
        rules: &'static [&'static str],
        keep: G,
        snapshot: Option<S>,
        consumed: usize,
        reach: usize,
    ) where
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let end = self.pos + consumed;
        let reach = if !TRACK {
            end
        } else if reach == usize::MAX {
            // Nothing of it failed: the diagnosing pass records nothing.
            return;
        } else {
            self.visible - reach
        };
        if reach + self.k < end {
            // Behind where the next step starts: an error there beats it.
            return;
        }
        // The state goes in as `record_chunk` puts it in: put back under
        // `Replay`, as the fast pass left it under `ReplayInPlace`.
        let state = match snapshot {
            Some(s) => s,
            None => self.context().user_state.clone(),
        };
        self.pending.push(Pending {
            run: keep(),
            rules,
            state,
            what: Replay::Chunk(self.pos),
            reach,
        });
    }

    /// The end of a fold the fast pass found, kept to be parsed again as
    /// `Stop::Ended` did: the last item taken and those before it whose
    /// failures may reach the end (oldest first), then the item that did not
    /// come, numbered `n + 1`. `reach` is how far that item's failures got,
    /// in `buf`, where the grammar tracks it.
    fn defer_ended<G>(
        &mut self,
        rules: &'static [&'static str],
        n: usize,
        keep: G,
        reach: Option<usize>,
    ) where
        G: FnOnce() -> Replayer<'c, K, S>,
    {
        let mut items = Ended {
            earlier: Vec::new(),
            prev: None,
            next: (self.pos, n + 1),
        };
        if let Some(prev) = self.prev_item {
            self.prune();
            items.earlier.extend(
                self.reaching
                    .iter()
                    .filter(|r| r.start < prev)
                    .map(|r| (r.start, r.n)),
            );
            items.prev = Some((prev, n));
        }
        let reach = reach.unwrap_or(self.pos).max(self.pos);
        let state = self.context().user_state.clone();
        self.pending.push(Pending {
            run: keep(),
            rules,
            state,
            what: Replay::Ended(items),
            reach,
        });
    }

    /// Takes the records of every pending step now, oldest first, as the
    /// parse in memory took them on its way here: a failure has come, or a
    /// cut sets aside what was recorded. The context comes out as it went
    /// in, user state, recoveries and emits alike.
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        self.prune();
        let pending = std::mem::take(&mut self.pending);
        let saved = {
            let c = self.context();
            (c.recoveries, c.recovered.len())
        };
        let emitted = self.context().emits.set_aside();
        let keep = self.context().user_state.clone();
        let last = self.eof && self.visible == filled!(self);
        let pass = Pass {
            diagnose: true,
            last,
            replay: true,
        };
        for p in pending {
            let Pending {
                mut run,
                rules,
                state,
                what,
                ..
            } = p;
            let mut run =
                |input: &mut Step<'_, '_, K, S>, pass: &Pass, n: usize| run.rerun(input, pass, n);
            self.context().user_state = state;
            match what {
                Replay::Chunk(start) => {
                    let before = (self.furthest.clone(), self.cut_stash.len());
                    let (_, touched) = self.attempt(start, rules, &mut run, pass, None);
                    // Over input that is not all there it records nothing.
                    if touched && !last {
                        self.furthest = before.0;
                        self.cut_stash.truncate(before.1);
                    }
                }
                Replay::Ended(items) => {
                    for (start, n) in items.iter() {
                        let _ = self.attempt(start, rules, &mut run, pass, Some(n));
                    }
                }
            }
            self.roll_back(None, saved);
            self.context().emits.discard();
        }
        self.context().user_state = keep;
        self.context().emits.restore(emitted);
    }

    /// Before a step that leaves a cut open sets aside what was recorded
    /// (`end_step`), what is pending is recorded: in memory the cut's
    /// `commit` takes it too. The step's records are set aside meanwhile.
    /// Called only where both hold: a cut is open and something is pending.
    #[cold]
    fn flush_before_cut(&mut self) {
        let c = self.context();
        let open = std::mem::take(&mut c.cut_stash);
        let recorded = c.furthest.take();
        self.flush();
        let c = self.context();
        c.cut_stash = open;
        c.furthest = recorded;
    }

    /// Forgets the items whose failures lie behind the next step: an error
    /// from there on is further, and beats them.
    fn prune(&mut self) {
        let (pos, k) = (self.pos, self.k);
        self.reaching.retain(|r| r.reach + k >= pos);
        if !self.pending.is_empty() {
            self.pending.retain(|p| p.reach + k >= pos);
        }
    }

    /// The items taken whose failures may reach the next step, and the last
    /// item taken, parsed again by the diagnosing pass for what they record,
    /// oldest first as the parse in memory records them: a failure they
    /// looked past may be the one an error after them has to beat.
    fn replay_prev<T, F>(
        &mut self,
        rules: &[&'static str],
        f: &mut F,
        item: Option<usize>,
        last: bool,
    ) where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<T, Fail>,
    {
        let (Some(prev), Some(n)) = (self.prev_item, item) else {
            return;
        };
        let saved = {
            let c = self.context();
            (c.recoveries, c.recovered.len())
        };
        let emitted = self.context().emits.set_aside();
        let pass = Pass {
            diagnose: true,
            last,
            replay: true,
        };
        self.prune();
        let earlier: Vec<Reaching> = self
            .reaching
            .iter()
            .filter(|r| r.start < prev)
            .copied()
            .collect();
        for r in earlier {
            let _ = self.attempt_held(r.start, rules, f, pass, Some(r.n));
        }
        let _ = self.attempt_held(prev, rules, f, pass, Some(n.saturating_sub(1)));
        self.roll_back(None, saved);
        self.context().emits.restore(emitted);
    }

    /// Runs `f` once over the window: the context goes in with its positions
    /// moved to the window's, and comes out with them moved back. An error
    /// of the diagnosing pass comes out finished - labelled with the rules of
    /// the path, merged with what was recorded, at its stream position; the
    /// end of a fold is recorded rather than returned.
    ///
    /// The context is lent to the step ([`Step`]), not moved in and back.
    #[allow(clippy::type_complexity)]
    fn attempt<T, F>(
        &mut self,
        start: usize,
        rules: &[&'static str],
        f: &mut F,
        pass: Pass,
        item: Option<usize>,
    ) -> (Result<(T, usize), Fail>, bool)
    where
        F: for<'w, 'x> FnMut(&mut Step<'w, 'x, K, S>, &Pass, usize) -> Result<T, Fail>,
    {
        let origin = self.base + start;
        let visible = self.visible;
        let ctx = self
            .ctx
            .as_deref_mut()
            .expect("the context is back between steps");
        let recovered = begin_attempt(ctx, origin, rules, &pass, item);
        // The view ends here, before the stream takes the context back.
        let (r, touched) = {
            let mut input = InputOf {
                input: winnow::stream::LocatingSlice::new(K::view(&bytes!(self)[start..visible])),
                state: ctx,
            };
            let r = f(&mut input, &pass, item.unwrap_or(0));
            let consumed = input.current_token_start();
            let r = attempt_result(r, consumed, item, rules, input.state);
            (r, input.state.touched)
        };
        (self.end_attempt(r, origin, recovered), touched)
    }

    /// [`Stream::attempt`] for the item loop's `f`, which holds the context
    /// ([`Held`]): moved in and back, as the loop does once a window. Only
    /// the diagnosing pass of an item and its replays run it.
    #[allow(clippy::type_complexity)]
    fn attempt_held<T, F>(
        &mut self,
        start: usize,
        rules: &[&'static str],
        f: &mut F,
        pass: Pass,
        item: Option<usize>,
    ) -> (Result<(T, usize), Fail>, bool)
    where
        F: for<'w> FnMut(&mut Held<'w, K, S>, &Pass, usize) -> Result<T, Fail>,
    {
        let origin = self.base + start;
        let ctx = self.ctx.take().expect("the context is back between steps");
        let recovered = begin_attempt(ctx, origin, rules, &pass, item);
        // The view ends here, before the context goes back to the stream.
        let (r, touched, ctx) = {
            let mut hold = Hold::new(ctx, K::view(&bytes!(self)[start..self.visible]));
            let input: &mut Held<'_, K, S> = &mut hold.input;
            let r = f(input, &pass, item.unwrap_or(0));
            let consumed = input.current_token_start();
            let r = attempt_result(r, consumed, item, rules, &mut input.state);
            let touched = input.state.touched;
            (r, touched, hold.put_back())
        };
        self.ctx = Some(ctx);
        (self.end_attempt(r, origin, recovered), touched)
    }

    /// What every attempt does once its step returned: the context's
    /// positions back to the stream's, what was recorded merged.
    fn end_attempt<T>(
        &mut self,
        r: Result<(T, usize), Fail>,
        origin: usize,
        recovered: usize,
    ) -> Result<(T, usize), Fail> {
        let ctx = self
            .ctx
            .as_deref_mut()
            .expect("the context is back between steps");
        ctx.rules.clear();
        ctx.origin = 0;
        if !ctx.cut_stash.is_empty() && !self.pending.is_empty() {
            self.flush_before_cut();
        }
        let ctx = self
            .ctx
            .as_deref_mut()
            .expect("the context is back between steps");
        end_step(&mut self.furthest, &mut self.cut_stash, ctx, origin);
        for e in &mut ctx.recovered[recovered..] {
            *e = shift(e.clone(), origin);
        }
        let best = |e: ParseError| match &self.furthest {
            Some(f) => f.clone().merge(shift(e, origin)),
            None => shift(e, origin),
        };
        r.map_err(|fail| match fail {
            Fail::Backtrack(e) => Fail::Backtrack(e.map(best)),
            Fail::Cut(e) => Fail::Cut(e.map(best)),
            Fail::Stop => Fail::Stop,
        })
    }

    fn absolute(&self, e: ParseError) -> ParseError {
        shift(e, self.base + self.pos)
    }

    fn roll_back(&mut self, snapshot: Option<S>, (recoveries, recovered): (usize, usize)) {
        let c = self.context();
        if let Some(s) = snapshot {
            c.user_state = s;
        }
        c.recoveries = recoveries;
        c.recovered.truncate(recovered);
    }

    fn too_far(&self) -> ParseError {
        reader::at(
            self.base + self.pos,
            format_args!(
                "the parse looked more than {} bytes ahead of where this step began; the input \
                 read so far does not decide it (raise `Reading::max_frame`)",
                self.max_frame
            ),
        )
    }

    /// One read, of at least `want` bytes asked (at least a block), after
    /// dropping what lies before the next step when that is a block or more.
    fn read_once(&mut self, want: usize) -> Result<(), ParseError> {
        if WHOLE {
            // All in memory: the one read is to the end, and drops nothing.
            self.eof = true;
            return self.validate();
        }
        self.prune();
        let oldest = self.reaching.first().map_or(self.pos, |r| r.start);
        let pending = self
            .pending
            .iter()
            .map(Pending::start)
            .min()
            .unwrap_or(self.pos);
        let keep = self
            .prev_item
            .unwrap_or(self.pos)
            .min(self.pos)
            .min(oldest)
            .min(pending);
        let keep = self.buf.droppable(keep, self.max_frame);
        if keep >= self.block {
            self.buf.drop_front(keep);
            self.base += keep;
            self.visible -= keep;
            self.pos -= keep;
            self.prev_item = self.prev_item.map(|p| p - keep);
            for r in &mut self.reaching {
                r.start -= keep;
                r.reach -= keep;
            }
            for p in &mut self.pending {
                p.shift_back(keep);
            }
        }
        let len = filled!(self);
        loop {
            match self.buf.read_from(&mut self.reader, want.max(self.block)) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    return Err(reader::at(
                        self.base + len,
                        format_args!("read failed: {e}"),
                    ));
                }
            }
        }
        self.validate()
    }

    /// Moves `visible` over what has arrived: for text, over what is checked
    /// as UTF-8, up to the last whole character.
    fn validate(&mut self) -> Result<(), ParseError> {
        if !K::TEXT {
            self.visible = filled!(self);
            return Ok(());
        }
        match std::str::from_utf8(&bytes!(self)[self.visible..]) {
            Ok(_) => self.visible = filled!(self),
            Err(e) => {
                self.visible += e.valid_up_to();
                if e.error_len().is_some() || self.eof {
                    let mut err = reader::at(self.base + self.visible, "invalid UTF-8");
                    err.invalid_utf8 = Some(bytes!(self)[self.visible]);
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

impl<'c, K: Kind, S: Clone + std::fmt::Debug, const TRACK: bool>
    Stream<'c, K, std::io::Empty, S, TRACK, true>
{
    /// A stream over an input that is all in memory: the steps read it
    /// where it is, nothing is copied, and every step knows it has the rest
    /// of the input (`Pass::last`) and runs the plain parsers. The
    /// generated `parse_<rule>_slice` and `parse_<rule>_slice_each` use it,
    /// through [`slice_passes`].
    ///
    /// Only `Diagnose::Off` and `Diagnose::Eager`: the plain parsers note
    /// no reach, so a step kept for a diagnosis that may come
    /// (`Pending`) would not know which items to parse again.
    /// [`slice_passes`] runs the two passes of the other modes.
    ///
    /// `checked`: the input is known to be UTF-8 - it came as a `&str` - so
    /// a text stream does not check it again.
    pub fn whole(
        input: &'c [u8],
        checked: bool,
        context: &'c mut ParseContext<S>,
        k: usize,
    ) -> Self {
        debug_assert!(
            matches!(context.diagnose, Diagnose::Off | Diagnose::Eager),
            "a whole stream runs one pass: {:?}",
            context.diagnose
        );
        let mut stream = Stream::new(
            std::io::empty(),
            context,
            super::Reading {
                block: 1 << 16,
                max_frame: usize::MAX / 4,
            },
            k,
        );
        stream.src = input;
        if checked {
            stream.visible = input.len();
            stream.eof = true;
        }
        stream
    }
}

impl<K: Kind, R: Read, S: Clone + std::fmt::Debug, const TRACK: bool, const WHOLE: bool> Drop
    for Stream<'_, K, R, S, TRACK, WHOLE>
{
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            // The caller's context takes no emits after the parse.
            ctx.emits.close();
        }
    }
}

/// The error an action of the path returned between two steps.
pub fn action_error(e: ErrMode<ParseError>) -> ParseError {
    match e {
        ErrMode::Backtrack(e) | ErrMode::Cut(e) => e,
        ErrMode::Incomplete(_) => ParseError::undiagnosed(),
    }
}

/// What every attempt does before its step runs: the context's positions
/// moved to the window's, what earlier steps recorded set aside. Returns
/// where the step's recoveries begin.
#[inline]
fn begin_attempt<S>(
    ctx: &mut ParseContext<S>,
    origin: usize,
    rules: &[&'static str],
    pass: &Pass,
    item: Option<usize>,
) -> usize {
    let recovered = ctx.recovered.len();
    ctx.touched = false;
    ctx.trivia_memo = TRIVIA_UNSEEN;
    ctx.last_trivia = (usize::MAX, usize::MAX);
    // What earlier steps recorded stays in stream positions and is
    // merged outside: moved into the window, a failure that began before
    // it would lose the order of where attempts began.
    ctx.furthest = None;
    // `end_step` emptied it when the last step ended.
    debug_assert!(ctx.cut_stash.is_empty());
    ctx.origin = origin;
    ctx.rules.clear();
    if pass.diagnose {
        ctx.rules.extend_from_slice(rules);
    }
    if item.is_some() {
        // An item hands its emits over inside `f`, when it is taken.
        ctx.emits.discard();
    }
    recovered
}

/// What a step's result is to the stream: with what it consumed, an item's
/// backtrack recorded, an error labelled with the rules of the path.
#[inline]
fn attempt_result<T, S>(
    r: Result<T, Fail>,
    consumed: usize,
    item: Option<usize>,
    rules: &[&'static str],
    ctx: &mut ParseContext<S>,
) -> Result<(T, usize), Fail> {
    match r {
        Ok(v) => Ok((v, consumed)),
        Err(Fail::Stop) => Err(Fail::Stop),
        Err(Fail::Backtrack(Some(e))) if item.is_some() => {
            let e = e.item(item.unwrap_or(0));
            e.record(ctx, 0);
            Err(Fail::Backtrack(None))
        }
        Err(fail) => {
            let cut = matches!(fail, Fail::Cut(_));
            let e = fail.error().map(|mut e| {
                for r in rules.iter().rev() {
                    e.push_rule(r);
                }
                e
            });
            Err(if cut {
                Fail::Cut(e)
            } else {
                Fail::Backtrack(e)
            })
        }
    }
}

/// What an eager pass recorded so far, kept aside: only that pass needs it.
#[cold]
#[inline(never)]
fn saved_furthest<S>(ctx: &ParseContext<S>) -> Option<ParseError> {
    ctx.furthest.clone()
}

/// What a step hands back when it ends, its context's records at its
/// `origin`: the records go to the stream's `furthest`. When the step left a
/// cut open - an item or a step that failed after its cut, or a prefix whose
/// cut holds into later steps - everything recorded before that cut goes to
/// the stream's `cut_stash` first, the earlier steps' records included: in
/// memory the cut's `commit` took all of `furthest`, wherever it came from.
/// Fields rather than `&mut Stream`: the step's view still borrows the
/// buffer here.
fn end_step<S>(
    furthest: &mut Option<ParseError>,
    cut_stash: &mut Vec<Option<ParseError>>,
    ctx: &mut ParseContext<S>,
    origin: usize,
) {
    if !ctx.cut_stash.is_empty() {
        set_aside(furthest, cut_stash, &mut ctx.cut_stash, origin);
    }
    let recorded = ctx.furthest.take().map(|e| shift(e, origin));
    *furthest = match (furthest.take(), recorded) {
        (Some(a), Some(b)) => Some(a.merge(b)),
        (a, b) => a.or(b),
    };
}

/// [`end_step`] with a cut still open. Out of line: in the loop over the
/// items it cost every item a dozen instructions inlined, open cut or not.
#[cold]
#[inline(never)]
fn set_aside(
    furthest: &mut Option<ParseError>,
    cut_stash: &mut Vec<Option<ParseError>>,
    open: &mut Vec<Option<ParseError>>,
    origin: usize,
) {
    cut_stash.push(furthest.take());
    cut_stash.extend(open.drain(..).map(|e| e.map(|e| shift(e, origin))));
}

/// What a try that touched the end of its window left open goes, out of
/// line for the same reason as [`set_aside`].
#[inline]
fn drop_open_cuts(open: &mut Vec<Option<ParseError>>) {
    #[cold]
    #[inline(never)]
    fn drop_them(open: &mut Vec<Option<ParseError>>) {
        open.clear();
    }
    if !open.is_empty() {
        drop_them(open);
    }
}

fn shift(mut e: ParseError, by: usize) -> ParseError {
    if e.is_undiagnosed() {
        return e;
    }
    e.offset += by;
    if let Some(b) = e.begun_at {
        e.begun_at = Some(b + by);
    }
    e
}
