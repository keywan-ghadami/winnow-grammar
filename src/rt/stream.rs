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
//! rule; each step is a [`Stream::chunk`] or a [`Stream::item`]. Between two
//! steps no view into the input is alive, so the input already parsed is
//! dropped and the next block read. Positions are moved to the stream's own:
//! errors, what the diagnosing pass recorded, spans (`ParseContext::origin`).

use super::{reader, Input, Slice, TRIVIA_UNSEEN};
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
pub fn edge<B: Slice, S: Clone + std::fmt::Debug, O, P, E>(
    k: usize,
    mut p: P,
) -> impl FnMut(&mut Input<B, S>) -> Result<O, ErrMode<E>>
where
    P: Parser<Input<B, S>, O, ErrMode<E>>,
{
    move |input| {
        let before = input.eof_offset();
        let r = p.parse_next(input);
        let left = match r {
            Ok(_) => input.eof_offset(),
            Err(_) => before,
        };
        if left < k {
            input.state.touched = true;
        }
        r
    }
}

/// [`edge`] for a quoted literal - `string`, whose scan for its closing
/// quote has no bound: a failure is the end's doing when no closing `quote`
/// follows the opening one in what the parser was given.
#[inline(always)]
pub fn edge_quoted<B: Slice, S: Clone + std::fmt::Debug, O, P, E>(
    k: usize,
    quote: u8,
    mut p: P,
) -> impl FnMut(&mut Input<B, S>) -> Result<O, ErrMode<E>>
where
    P: Parser<Input<B, S>, O, ErrMode<E>>,
{
    let mut p = edge(k, move |i: &mut Input<B, S>| p.parse_next(i));
    move |input| {
        let cp = input.checkpoint();
        let r = p.parse_next(input);
        if r.is_err() {
            input.reset(&cp);
            let b = input.as_bstr();
            if b.first() == Some(&quote) && !closes(&b[1..], quote) {
                input.state.touched = true;
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
pub fn touch<B: Slice, S: Clone + std::fmt::Debug>(input: &mut Input<B, S>) {
    input.state.touched = true;
}

/// A span in the input of a step, moved to the whole input's positions.
#[inline(always)]
pub fn span_at<B: Slice, S: Clone + std::fmt::Debug>(
    input: &Input<B, S>,
    span: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let o = input.state.origin;
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
}

/// How a step failed, with the error of the diagnosing pass.
#[derive(Debug)]
pub enum Fail {
    Backtrack(Option<ParseError>),
    Cut(Option<ParseError>),
}

impl Fail {
    fn error(self) -> Option<ParseError> {
        match self {
            Fail::Backtrack(e) | Fail::Cut(e) => e,
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
pub fn commit<B: Slice, S: Clone + std::fmt::Debug>(input: &Input<B, S>, pass: &Pass) -> bool {
    !pass.replay && input.current_token_start() > 0 && (pass.last || !input.state.touched)
}

/// The reader, the buffer and the context of a stream parse. See the module
/// documentation; the generated `parse_<rule>_reader` is its only user.
pub struct Stream<'c, K: Kind, R: Read, S: Clone + std::fmt::Debug> {
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
    /// The stream's offset of `buf[0]`.
    base: usize,
    /// The end of what a step may see: for text, of what is checked as UTF-8.
    visible: usize,
    eof: bool,
    ctx: Option<ParseContext<S>>,
    out: &'c mut ParseContext<S>,
    /// What the diagnosing pass recorded, at stream positions.
    furthest: Option<ParseError>,
    _kind: PhantomData<K>,
}

impl<'c, K: Kind, R: Read, S: Clone + std::fmt::Debug> Stream<'c, K, R, S> {
    /// A stream over `reader`, parsing with a copy of `context`, which gets
    /// the copy back - user state, recoveries and all - when the stream is
    /// dropped. `k` is the grammar's own lookahead.
    pub fn new(
        reader: R,
        context: &'c mut ParseContext<S>,
        reading: super::Reading,
        k: usize,
    ) -> Self {
        let mut ctx = context.clone();
        ctx.begin_parse();
        Stream {
            reader,
            block: reading.block.max(1),
            lookahead: reading.max_frame.saturating_add(k),
            max_frame: reading.max_frame,
            buf: reader::Buf::default(),
            pos: 0,
            prev_item: None,
            base: 0,
            visible: 0,
            eof: false,
            ctx: Some(ctx),
            out: context,
            furthest: None,
            _kind: PhantomData,
        }
    }

    /// The context, for the actions the driver runs between steps.
    pub fn context(&mut self) -> &mut ParseContext<S> {
        self.ctx
            .as_mut()
            .expect("the context is back between steps")
    }

    /// One step: `f` parses a prefix or a suffix, whose value does not
    /// borrow the input. `rules` are the rules of the path the step is
    /// inside, outermost first.
    pub fn chunk<T, F>(&mut self, rules: &[&'static str], mut f: F) -> Result<T, ParseError>
    where
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<T, Fail>,
    {
        let r = self.run(rules, &mut f, None);
        match self.finished(r)? {
            Some(v) => Ok(v),
            None => unreachable!("a chunk is not an item"),
        }
    }

    /// One item of the fold: `f` parses it and, where [`commit`] says so,
    /// hands it to the step, answering whether it did. `false` from here is
    /// the end of the fold, at the position before the item; `index` numbers
    /// the item in an error, from 1.
    pub fn item<F>(
        &mut self,
        rules: &[&'static str],
        index: usize,
        mut f: F,
    ) -> Result<bool, ParseError>
    where
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<bool, Fail>,
    {
        let r = self.run(rules, &mut f, Some(index));
        Ok(self.finished(r)?.unwrap_or(false))
    }

    /// The end: `f` skips what the entry point skips after its rule, and then
    /// nothing may be left.
    pub fn finish<F>(&mut self, f: F) -> Result<(), ParseError>
    where
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<(), Fail>,
    {
        self.chunk(&[], f)?;
        while self.visible == self.pos && !self.eof {
            self.read_once(0)?;
        }
        if self.visible == self.pos && self.buf.len() == self.pos {
            return Ok(());
        }
        if self.context().diagnose == Diagnose::Off {
            return Err(ParseError::undiagnosed());
        }
        let mut ctx = self.ctx.take().expect("the context is back between steps");
        ctx.furthest = self.local_furthest(self.base + self.pos);
        let (e, ctx) = {
            let input = Input {
                input: winnow::stream::LocatingSlice::new(K::view(
                    &self.buf.bytes()[self.pos..self.visible],
                )),
                state: ctx,
            };
            let e = ParseError::from_stream(&input).add_expected("end of input");
            (input.state.best(e), input.state)
        };
        self.ctx = Some(ctx);
        let e = self.absolute(e);
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
            e.found = crate::error::next_word(&self.buf.bytes()[at..self.visible]);
        }
        Err(e)
    }

    fn run<T, F>(
        &mut self,
        rules: &[&'static str],
        f: &mut F,
        item: Option<usize>,
    ) -> Result<Option<T>, ParseError>
    where
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<T, Fail>,
    {
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
            let last = self.eof && self.visible == self.buf.len();
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
            let diagnose = Pass {
                diagnose: true,
                last,
                replay: false,
            };
            let (r, touched) = self.attempt(self.pos, rules, f, first, item);
            if touched && !last {
                if full {
                    return Err(self.too_far());
                }
                self.roll_back(snapshot, saved);
                let more = window.max(self.block);
                let before = self.visible;
                while self.visible == before && !self.eof {
                    self.read_once(more)?;
                }
                continue;
            }
            return match r {
                Ok((v, consumed)) => {
                    if item.is_some() && consumed > 0 {
                        self.prev_item = Some(self.pos);
                    }
                    self.pos += consumed;
                    Ok(Some(v))
                }
                Err(Fail::Backtrack(_)) if item.is_some() => {
                    // The end of the fold. What the item expected is kept for
                    // an error further on, as the fold keeps it - and so is
                    // what the item before it recorded, which the parse in
                    // memory would have recorded on its way here.
                    if !first.diagnose && mode != Diagnose::Off {
                        let keep = self.context().user_state.clone();
                        self.replay_prev(rules, f, item, last);
                        let _ = self.attempt(self.pos, rules, f, diagnose, item);
                        self.context().user_state = keep;
                        self.roll_back(None, saved);
                    }
                    Ok(None)
                }
                Err(fail) if first.diagnose => {
                    Err(fail.error().unwrap_or_else(ParseError::undiagnosed))
                }
                Err(_) if mode == Diagnose::Off => Err(ParseError::undiagnosed()),
                Err(_) => {
                    if let Some(s) = snapshot {
                        self.context().user_state = s;
                    }
                    if item.is_some() {
                        let keep = self.context().user_state.clone();
                        self.replay_prev(rules, f, item, last);
                        self.context().user_state = keep;
                    }
                    match self.attempt(self.pos, rules, f, diagnose, item).0 {
                        Ok((v, consumed)) => {
                            self.pos += consumed;
                            Ok(Some(v))
                        }
                        Err(fail) => Err(fail.error().unwrap_or_else(ParseError::undiagnosed)),
                    }
                }
            };
        }
    }

    /// The last item taken, parsed again by the diagnosing pass for what it
    /// records: a failure it looked past may be the one an error after it
    /// has to beat.
    fn replay_prev<T, F>(
        &mut self,
        rules: &[&'static str],
        f: &mut F,
        item: Option<usize>,
        last: bool,
    ) where
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<T, Fail>,
    {
        if let (Some(at), Some(n)) = (self.prev_item, item) {
            let saved = {
                let c = self.context();
                (c.recoveries, c.recovered.len())
            };
            let pass = Pass {
                diagnose: true,
                last,
                replay: true,
            };
            let _ = self.attempt(at, rules, f, pass, Some(n.saturating_sub(1)));
            self.roll_back(None, saved);
        }
    }

    /// Runs `f` once over the window: the context goes in with its positions
    /// moved to the window's, and comes out with them moved back. An error
    /// of the diagnosing pass comes out finished - labelled with the rules of
    /// the path, merged with what was recorded, at its stream position; the
    /// end of a fold is recorded rather than returned.
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
        F: for<'w> FnMut(&mut Input<K::View<'w>, S>, &Pass) -> Result<T, Fail>,
    {
        let origin = self.base + start;
        let mut ctx = self.ctx.take().expect("the context is back between steps");
        let recovered = ctx.recovered.len();
        ctx.touched = false;
        ctx.trivia_memo = TRIVIA_UNSEEN;
        ctx.last_trivia = (usize::MAX, usize::MAX);
        ctx.furthest = self.local_furthest(origin);
        ctx.origin = origin;
        ctx.rules.clear();
        if pass.diagnose {
            ctx.rules.extend_from_slice(rules);
        }
        let mut input = Input {
            input: winnow::stream::LocatingSlice::new(K::view(
                &self.buf.bytes()[start..self.visible],
            )),
            state: ctx,
        };
        let r = f(&mut input, &pass);
        let consumed = input.current_token_start();
        let r = match r {
            Ok(v) => Ok((v, consumed)),
            Err(Fail::Backtrack(Some(e))) if item.is_some() => {
                let e = e.item(item.unwrap_or(0));
                e.record(&mut input.state, 0);
                Err(Fail::Backtrack(None))
            }
            Err(fail) => {
                let cut = matches!(fail, Fail::Cut(_));
                let e = fail.error().map(|mut e| {
                    for r in rules.iter().rev() {
                        e.push_rule(r);
                    }
                    input.state.best(e)
                });
                Err(if cut {
                    Fail::Cut(e)
                } else {
                    Fail::Backtrack(e)
                })
            }
        };
        let touched = input.state.touched;
        let mut ctx = input.state;
        ctx.rules.clear();
        ctx.origin = 0;
        self.furthest = ctx.furthest.take().map(|e| shift(e, origin));
        for e in &mut ctx.recovered[recovered..] {
            *e = shift(e.clone(), origin);
        }
        self.ctx = Some(ctx);
        let r = r.map_err(|fail| match fail {
            Fail::Backtrack(e) => Fail::Backtrack(e.map(|e| shift(e, origin))),
            Fail::Cut(e) => Fail::Cut(e.map(|e| shift(e, origin))),
        });
        (r, touched)
    }

    /// What was recorded, at the positions of a window that starts at
    /// stream position `origin`; what lies before it cannot win on progress
    /// and is dropped.
    fn local_furthest(&self, origin: usize) -> Option<ParseError> {
        self.furthest.clone().and_then(|mut e| {
            if e.offset < origin {
                return None;
            }
            e.offset -= origin;
            if let Some(b) = e.begun_at {
                e.begun_at = Some(b.saturating_sub(origin));
            }
            Some(e)
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
        let keep = self.prev_item.unwrap_or(self.pos).min(self.pos);
        if keep >= self.block {
            self.buf.drop_front(keep);
            self.base += keep;
            self.visible -= keep;
            self.pos -= keep;
            self.prev_item = self.prev_item.map(|p| p - keep);
        }
        let len = self.buf.len();
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
            self.visible = self.buf.len();
            return Ok(());
        }
        match std::str::from_utf8(&self.buf.bytes()[self.visible..]) {
            Ok(_) => self.visible = self.buf.len(),
            Err(e) => {
                self.visible += e.valid_up_to();
                if e.error_len().is_some() || self.eof {
                    let mut err = reader::at(self.base + self.visible, "invalid UTF-8");
                    err.invalid_utf8 = Some(self.buf.bytes()[self.visible]);
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

impl<K: Kind, R: Read, S: Clone + std::fmt::Debug> Drop for Stream<'_, K, R, S> {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            *self.out = ctx;
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
