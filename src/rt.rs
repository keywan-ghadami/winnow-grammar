//! Runtime helpers for the generated code - the places where the
//! diagnostics engine ([`crate::ParseError`]) steps in.
//!
//! Everything here works concretely on [`ParseInput`] because it needs the
//! state (`input.state`): that is where the furthest failure position lives,
//! which a successful backtrack would otherwise discard. The error type is
//! generic ([`RtError`](crate::rt::RtError)): the same helper serves the fast pass with
//! `EmptyError` and the diagnosing pass with [`ParseError`] - see ADR 17.

use crate::ascii::AsciiClass;
use crate::error::{Diagnostics, ParseError};
use crate::{Diagnose, FoldProgress, ParseInput};
use winnow::error::{AddContext, EmptyError, ErrMode, ParserError, StrContext};
use winnow::stream::{AsBStr, FindSlice, Location, Stream};
use winnow::Parser;

/// The bounds the generated code puts on its error type parameter `E`:
/// winnow's own (`ParserError`; `AddContext` for `.context(..)`) and
/// [`Diagnostics`] for the helpers here. A blanket impl: [`ParseError`] and
/// winnow's `EmptyError` satisfy it, nothing implements it by hand.
pub trait RtError<'a, S>:
    ParserError<ParseInput<'a, S>> + AddContext<ParseInput<'a, S>, StrContext> + Diagnostics
where
    S: Clone + std::fmt::Debug,
{
}

impl<'a, S, E> RtError<'a, S> for E
where
    S: Clone + std::fmt::Debug,
    E: ParserError<ParseInput<'a, S>> + AddContext<ParseInput<'a, S>, StrContext> + Diagnostics,
{
}

/// `until("lit")` - consume everything before the next occurrence of a literal
/// terminator, without consuming the terminator itself.
///
/// The scan is the point: `find_slice` searches the raw input a machine word at
/// a time (memchr, with SIMD where the target has it and the same trick in
/// portable code where it does not), instead of running the terminator's parser
/// once per character. Byte-by-byte, a rule like `until(";")` over a large
/// input costs a parser call per byte, and a `recover` that has to skip a long
/// stretch is quadratic.
///
/// Never fails: with no terminator in the rest of the input it consumes to the
/// end, which is what a repetition-based `until` did.
pub fn scan_to_literal<'a, S: Clone + std::fmt::Debug, E>(
    needle: &'static str,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let end = match input.find_slice(needle) {
            Some(range) => range.start,
            None => input.eof_offset(),
        };
        Ok(input.next_slice(end))
    }
}

/// `until(line_ending)` - consume the rest of the line, without consuming the
/// line ending.
///
/// Same scan as [`scan_to_literal`], plus the one thing a literal cannot
/// express: the terminator is two characters or one. Finding `\n` and then
/// looking at the byte before it is O(1) and settles `\r\n` correctly - a
/// scan for `"\r\n"` would run past an earlier bare `\n`, and a scan for
/// `"\n"` alone would leave the carriage return on the wrong side. A bare
/// `\r` is ordinary text, as it was before.
pub fn scan_to_line_ending<'a, S: Clone + std::fmt::Debug, E>(
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let end = match input.find_slice('\n') {
            Some(range) => {
                let n = range.start;
                // The text before the newline; if it ends in `\r`, that
                // carriage return belongs to the line ending, not the line.
                if input.peek_slice(n).ends_with('\r') {
                    n - 1
                } else {
                    n
                }
            }
            None => input.eof_offset(),
        };
        Ok(input.next_slice(end))
    }
}

/// `until(";" | "\n")` / `until(";" | line_ending)` - a terminator with two
/// or three fixed alternatives, found in one pass: `memchr2`/`memchr3` over
/// the alternatives' first bytes, then a check of the full string at each
/// candidate (winnow's tuple `find_slice`). `line_ending` contributes `\n`
/// and the one look back for `\r`.
///
/// The code generator calls this with two or three needles in total; one is
/// [`scan_to_literal`] / [`scan_to_line_ending`], and more than three take
/// the position-by-position path.
pub fn scan_to_any<'a, S: Clone + std::fmt::Debug, E>(
    lits: &'static [&'static str],
    line_ending: bool,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, ErrMode<E>> {
    let mut needles: Vec<&'static str> = lits.to_vec();
    if line_ending {
        needles.push("\n");
    }
    move |input| {
        let hit = match needles.as_slice() {
            [] => None,
            [a] => input.find_slice(*a),
            [a, b] => input.find_slice((*a, *b)),
            [a, b, c] => input.find_slice((*a, *b, *c)),
            _ => unreachable!("the code generator limits a scan set to three needles"),
        };
        let end = match hit {
            Some(range) => {
                let n = range.start;
                let before = input.peek_slice(n);
                let at_newline = input.peek_slice(range.end).ends_with('\n') && range.len() == 1;
                if line_ending && at_newline && before.ends_with('\r') {
                    n - 1
                } else {
                    n
                }
            }
            None => input.eof_offset(),
        };
        Ok(input.next_slice(end))
    }
}

/// How `parse_<rule>_pieces` runs the pieces of a `par_fold` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parallelism {
    /// No cut at all: the whole input, one parse. What `parse_<rule>()` does.
    Off,
    /// This many pieces.
    Pieces(usize),
    /// One piece per available core (`std::thread::available_parallelism`).
    #[default]
    Auto,
}

impl Parallelism {
    /// The number of pieces this asks for; `None` for [`Parallelism::Off`].
    pub fn pieces(self) -> Option<usize> {
        match self {
            Parallelism::Off => None,
            Parallelism::Pieces(n) => Some(n.max(1)),
            Parallelism::Auto => Some(
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1),
            ),
        }
    }
}

/// The driver behind `parse_<rule>_pieces` on a `par_fold` rule: cut `input`
/// at `boundary` into the pieces `how` asks for, parse each under a context
/// from `new_context`, and fold the results with `merge`, first piece first.
/// A piece's error is shifted to its offset in `input`, so it renders against
/// the whole input; when several pieces fail, the first one's error is the
/// one returned.
///
/// `fast` and `diagnose` are the rule's parser instantiated with `EmptyError`
/// and with [`ParseError`]; each piece goes through [`entry_framed`], so a
/// failing piece is diagnosed from the item its fast pass stopped in.
///
/// With the `rayon` feature the pieces are parsed on rayon's global pool;
/// without it, in sequence - the same cut and the same answer, which is what
/// lets a test check the split without threads.
#[cfg(not(feature = "rayon"))]
pub fn fold_pieces<'a, S, T, F, D, M, C>(
    input: &'a str,
    boundary: &str,
    how: Parallelism,
    new_context: C,
    fast: F,
    diagnose: D,
    merge: M,
) -> Result<T, ParseError>
where
    S: Clone + std::fmt::Debug,
    C: Fn() -> crate::ParseContext<S>,
    F: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<EmptyError>>,
    D: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<ParseError>>,
    M: Fn(T, T) -> T,
{
    let ranges = piece_ranges(input, boundary, how);
    let mut acc: Option<T> = None;
    for r in ranges {
        let v = parse_piece(input, r, &new_context, &fast, &diagnose)?;
        acc = Some(match acc {
            Some(a) => merge(a, v),
            None => v,
        });
    }
    Ok(acc.expect("at least one piece"))
}

/// See the sequential definition; this one runs the pieces on rayon's
/// global pool. Configure that pool (`rayon::ThreadPoolBuilder`) to bound
/// the threads; `Parallelism` bounds the pieces.
#[cfg(feature = "rayon")]
pub fn fold_pieces<'a, S, T, F, D, M, C>(
    input: &'a str,
    boundary: &str,
    how: Parallelism,
    new_context: C,
    fast: F,
    diagnose: D,
    merge: M,
) -> Result<T, ParseError>
where
    S: Clone + std::fmt::Debug,
    T: Send,
    C: Fn() -> crate::ParseContext<S> + Sync,
    F: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<EmptyError>> + Sync,
    D: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<ParseError>> + Sync,
    M: Fn(T, T) -> T,
{
    use rayon::prelude::*;
    let ranges = piece_ranges(input, boundary, how);
    let results: Vec<Result<T, ParseError>> = ranges
        .into_par_iter()
        .map(|r| parse_piece(input, r, &new_context, &fast, &diagnose))
        .collect();
    let mut acc: Option<T> = None;
    for res in results {
        let v = res?;
        acc = Some(match acc {
            Some(a) => merge(a, v),
            None => v,
        });
    }
    Ok(acc.expect("at least one piece"))
}

fn piece_ranges(input: &str, boundary: &str, how: Parallelism) -> Vec<std::ops::Range<usize>> {
    match how.pieces() {
        None => std::iter::once(0..input.len()).collect(),
        Some(n) => frames(input, boundary, n),
    }
}

fn parse_piece<'a, S, T, F, D, C>(
    input: &'a str,
    range: std::ops::Range<usize>,
    new_context: &C,
    fast: &F,
    diagnose: &D,
) -> Result<T, ParseError>
where
    S: Clone + std::fmt::Debug,
    C: Fn() -> crate::ParseContext<S>,
    F: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<EmptyError>>,
    D: Fn(&mut ParseInput<'a, S>) -> Result<T, ErrMode<ParseError>>,
{
    let start = range.start;
    let mut piece = ParseInput {
        input: winnow::stream::LocatingSlice::new(&input[range]),
        state: new_context(),
    };
    entry_framed(&mut piece, fast, diagnose).map_err(|mut e| {
        if !e.is_undiagnosed() {
            e.offset += start;
        }
        e
    })
}

/// The byte ranges of `n` pieces of `input`, each beginning right after a
/// `boundary` - the split behind `frames_<rule>()` on a `#[frame]` rule.
///
/// The input is divided into `n` equal byte ranges without looking at it.
/// Every piece but the first then moves its start forward to just past the
/// first boundary at or after that point, and ends where the next piece
/// starts; the last runs to the end. Hence:
///
/// * every frame lies in exactly one piece - the one that finds its *end*;
/// * a frame longer than a piece is not an error: the pieces whose repaired
///   start lands at or past their end come out empty, and the piece before
///   them parses through;
/// * input that does not end in a boundary still has its last frame in the
///   last piece, for the fold to accept or reject as the grammar says;
/// * `n == 0` is read as `1`, and empty input gives one empty piece.
///
/// The split works on bytes ([`frames_bytes`]); this is the `&str` view of
/// it. A valid UTF-8 boundary can only match at a character boundary, so
/// every range is one - a consequence of the input being `&str`, not a
/// constraint the split adds.
pub fn frames(input: &str, boundary: &str, n: usize) -> Vec<std::ops::Range<usize>> {
    frames_bytes(input.as_bytes(), boundary.as_bytes(), n)
}

/// [`frames`] on bytes: the split itself, with no notion of characters. The
/// generated parsers take `&str`, so [`frames`] is what they use; this is
/// the primitive underneath, for a byte-oriented input type to build on.
pub fn frames_bytes(input: &[u8], boundary: &[u8], n: usize) -> Vec<std::ops::Range<usize>> {
    let n = n.max(1);
    let len = input.len();

    let mut starts = Vec::with_capacity(n + 1);
    starts.push(0);
    for k in 1..n {
        // `len * k` in `usize` would overflow before `len` does on a 32-bit
        // target; the quotient itself is at most `len`.
        let nominal = ((len as u128 * k as u128) / n as u128) as usize;
        let tail: &[u8] = &input[nominal..];
        let start = tail
            .find_slice(boundary)
            .map(|r| nominal + r.end)
            .unwrap_or(len);
        starts.push(start);
    }
    starts.push(len);

    starts.windows(2).map(|w| w[0]..w[1]).collect()
}

/// `x?` - at most once. A failed attempt is **recorded**, not thrown away:
/// if the rule later fails at a shallower position or input is left over, it
/// is the better message.
pub fn opt_recording<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Option<O>, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    move |input| {
        let cp = input.checkpoint();
        let start = input.current_token_start();
        match p.parse_next(input) {
            Ok(v) => Ok(Some(v)),
            Err(ErrMode::Backtrack(e)) => {
                e.record(&mut input.state, start);
                input.reset(&cp);
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

/// `x*` / `x+` - repetition with a minimum count. The reason why it did not
/// continue is recorded and carries the index of the attempted element
/// (`in item 3`). Below the minimum count it is the error itself.
///
/// The open-ended case of [`repeat_recording_bounded`].
pub fn repeat_recording<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    min: usize,
    p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Vec<O>, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    repeat_recording_bounded(min, None, p)
}

/// `x{n}` / `x{n,}` / `x{n,m}` - repetition with explicit bounds, collecting
/// the elements.
///
/// Greedy and possessive, like [`repeat_recording`]: it takes as many elements
/// as it can up to `max` and never gives one back to help a later pattern
/// match. Below `min` the element's own error is the failure; at `max` the
/// repetition simply stops, and whatever follows sees the rest of the input.
///
/// [`repeat_counting_bounded`] is the same loop for a repetition whose
/// elements nobody names. The two are written out separately on purpose:
/// threading the accumulator through a closure so that one body could serve
/// both cost the collecting form 35% (`benches/repetition.rs`), and this is
/// the form every bound repetition in every grammar runs.
pub fn repeat_recording_bounded<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    min: usize,
    max: Option<usize>,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Vec<O>, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    // The bound as a plain number, tested once per element instead of an
    // `Option` unwrapped every time round.
    let cap = max.unwrap_or(usize::MAX);
    move |input| {
        let mut items = Vec::new();
        loop {
            if items.len() >= cap {
                break;
            }
            let cp = input.checkpoint();
            let start = input.current_token_start();
            match p.parse_next(input) {
                Ok(v) => {
                    if input.current_token_start() == start {
                        // The element matched without consuming anything, so
                        // repeating it can never make progress and the loop
                        // has to stop - but not before the minimum is
                        // reached, or `{n}` would quietly hand back fewer
                        // than `n` items. An empty match still counts:
                        // `("a"?){3}` matches the empty input three times,
                        // and each push moves the count towards `min`, so
                        // this terminates.
                        if items.len() < min {
                            items.push(v);
                            continue;
                        }
                        input.reset(&cp);
                        break;
                    }
                    items.push(v);
                }
                Err(ErrMode::Backtrack(e)) => {
                    let e = e.item(items.len() + 1);
                    if items.len() < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    e.record(&mut input.state, start);
                    input.reset(&cp);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(items)
    }
}

/// `x*` / `x+` with no binding, and `count(x)`: the repetition of
/// [`repeat_recording`], answering with how many elements there were instead
/// of with the elements.
///
/// The grammar has already said the elements are not wanted - it named none -
/// and over a large input holding them is the memory cost, not the parse. That
/// is the same reason [`fold_recording`] exists.
/// `tests/repetition_memory_test.rs` pins the memory; `benches/repetition.rs`
/// measures the time, which on a short repetition is about half.
///
/// The loop is [`repeat_recording_bounded`]'s, with the `Vec` replaced by a
/// counter - see the note there on why it is written out twice.
pub fn repeat_counting<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    min: usize,
    p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<usize, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    repeat_counting_bounded(min, None, p)
}

/// The bounded form of [`repeat_counting`].
pub fn repeat_counting_bounded<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    min: usize,
    max: Option<usize>,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<usize, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    // The bound as a plain number, tested once per element instead of an
    // `Option` unwrapped every time round.
    let cap = max.unwrap_or(usize::MAX);
    move |input| {
        let mut seen = 0usize;
        loop {
            if seen >= cap {
                break;
            }
            let cp = input.checkpoint();
            let start = input.current_token_start();
            match p.parse_next(input) {
                Ok(_) => {
                    if input.current_token_start() == start {
                        if seen < min {
                            seen += 1;
                            continue;
                        }
                        input.reset(&cp);
                        break;
                    }
                    seen += 1;
                }
                Err(ErrMode::Backtrack(e)) => {
                    let e = e.item(seen + 1);
                    if seen < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    e.record(&mut input.state, start);
                    input.reset(&cp);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(seen)
    }
}

/// `dec<T>(p)` - the text `p` matched, read as a `T`.
///
/// Not a faster way to parse a number: measured, it is what
/// [`repeat_recording_bounded`]'s text plus a fold in an action costs
/// (`benches/repetition.rs`). It is here so that the fold is not written out
/// at every numeric field, and because a value the format does not fit
/// becomes a parse error with the reason attached rather than a silent wrap.
///
/// The reason is why this is not winnow's `parse_to`: that discards the
/// `FromStr` error and reports a bare position, so "number too large to fit
/// in target type" would be lost.
pub fn dec<'a, S, T, P, E>(mut p: P) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<T, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
    P: Parser<ParseInput<'a, S>, &'a str, ErrMode<E>>,
    E: RtError<'a, S>,
{
    move |input| {
        let cp = input.checkpoint();
        let text = p.parse_next(input)?;
        match text.parse::<T>() {
            Ok(v) => Ok(v),
            Err(e) => {
                // The number is the whole of what was matched, so the error
                // belongs at its start, not after it.
                input.reset(&cp);
                Err(ErrMode::Backtrack(E::external(input, e)))
            }
        }
    }
}

/// `fold(pattern, init, step)` - a repetition that threads an accumulator
/// instead of collecting.
///
/// Identical to [`repeat_recording`] in how it handles progress, backtracking
/// and error recording; the difference is that nothing is ever pushed into a
/// `Vec`. That matters when the number of items is large enough that the
/// collection, not the parse, is the memory cost - a log or data file with
/// millions of records is summarised in constant space.
pub fn fold_recording<'a, S, O, Acc, P, I, F, E>(
    min: usize,
    p: P,
    init: I,
    step: F,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Acc, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
    I: FnMut() -> Acc,
    F: FnMut(Acc, O) -> Acc,
    E: RtError<'a, S>,
{
    fold_impl(min, p, init, step, false)
}

/// The fold in the body of a `par_fold` rule: [`fold_recording`] that also
/// leaves a trail. On every exit it writes where it stopped and how many
/// items it had accepted into [`ParseContext::fold`](crate::ParseContext::fold),
/// and it numbers its items from `fold.base` - so that a replay of the tail
/// of an input (see [`entry_framed`]) reports `in item 4711`, not `item 1`.
pub fn par_fold_recording<'a, S, O, Acc, P, I, F, E>(
    min: usize,
    p: P,
    init: I,
    step: F,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Acc, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
    I: FnMut() -> Acc,
    F: FnMut(Acc, O) -> Acc,
    E: RtError<'a, S>,
{
    fold_impl(min, p, init, step, true)
}

fn fold_impl<'a, S, O, Acc, P, I, F, E>(
    min: usize,
    mut p: P,
    mut init: I,
    mut step: F,
    tracked: bool,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Acc, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
    I: FnMut() -> Acc,
    F: FnMut(Acc, O) -> Acc,
    E: RtError<'a, S>,
{
    move |input| {
        let mut acc = init();
        let mut seen = 0usize;
        let base = if tracked { input.state.fold.base } else { 0 };
        // Where the fold stopped, for the replay - see `FoldProgress`.
        let stopped = |input: &mut ParseInput<'a, S>, seen: usize, at: usize| {
            if tracked {
                input.state.fold.seen = seen;
                input.state.fold.at = at;
            }
        };
        loop {
            let cp = input.checkpoint();
            let start = input.current_token_start();
            match p.parse_next(input) {
                Ok(v) => {
                    // Zero-progress guard: otherwise the loop spins forever
                    // when the element matches without consuming anything.
                    if input.current_token_start() == start {
                        input.reset(&cp);
                        stopped(input, seen, start);
                        break;
                    }
                    acc = step(acc, v);
                    seen += 1;
                }
                Err(ErrMode::Backtrack(e)) => {
                    let e = e.item(base + seen + 1);
                    stopped(input, seen, start);
                    if seen < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    e.record(&mut input.state, start);
                    input.reset(&cp);
                    break;
                }
                Err(e) => {
                    stopped(input, seen, start);
                    return Err(e);
                }
            }
        }
        Ok(acc)
    }
}

/// A labelled alternative (`# "…"`). If it fails at its starting position,
/// its name counts as the expectation instead of the internal message:
/// ``expected `(` `` becomes `expected function argument`. If it made
/// progress, its own message is the more informative one and stays.
pub fn labelled<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    label: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    move |input| {
        let start = input.current_token_start();
        match p.parse_next(input) {
            Err(ErrMode::Backtrack(e)) => Err(ErrMode::Backtrack(e.labelled(start, label))),
            r => r,
        }
    }
}

/// `intern(p)` and `ident`, for a grammar that declared an `interner` - ADR 22.
///
/// The same combinator as [`intern`], reaching the interner the grammar named
/// instead of the one in the context. It does *not* go through the context's
/// lookup cache: that cache belongs to the built-in interner, which it knows
/// how to invalidate; a declared interner caches as it sees fit, and the
/// interesting ones - a direct-index table whose slot number is the identity -
/// have nothing a cache would add.
pub fn intern_in<'a, I, S, O, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<crate::Symbol, ErrMode<E>>
where
    I: crate::Interner,
    S: Clone + std::fmt::Debug + crate::InternerOf<I>,
    O: AsRef<str>,
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    move |input| {
        let text = p.parse_next(input)?;
        Ok(crate::InternerOf::<I>::interner(&mut input.state.user_state).intern(text.as_ref()))
    }
}

/// `recover(body, sync)` - run `body`, and on failure skip to `sync` and carry
/// on, keeping what went wrong.
///
/// `alt((body.map(Some), (skip, sync).map(|_| None)))` was the shape before,
/// and it threw the body's error away where it was produced. Writing the two
/// branches out keeps it: the count goes into the context in both passes, the
/// error itself when the diagnosing engine is the one running.
///
/// A **cut** inside the body is not recovered from. That is what a cut is for -
/// the input is wrong rather than merely unexpected here - and it was already
/// so, `alt` not catching `ErrMode::Cut` either.
pub fn recover_recording<'a, S, O, Skipped, Synced, B, K, Y, E>(
    mut body: B,
    mut skip: K,
    mut sync: Y,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Option<O>, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    B: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
    K: Parser<ParseInput<'a, S>, Skipped, ErrMode<E>>,
    Y: Parser<ParseInput<'a, S>, Synced, ErrMode<E>>,
    E: RtError<'a, S>,
{
    move |input| {
        let cp = input.checkpoint();
        match body.parse_next(input) {
            Ok(v) => Ok(Some(v)),
            Err(ErrMode::Backtrack(e)) => {
                input.reset(&cp);
                skip.parse_next(input)?;
                sync.parse_next(input)?;
                input.state.record_recovery(e.into_parse_error());
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

/// `intern(p)` - run `p` and intern what it yields, in the interner the
/// context carries (ADR 14). The whole of `Symbol`'s value is that a parse
/// which sees the same text twice returns the same 4-byte id twice, so the
/// consumer compares ids instead of strings.
///
/// `ident` is `intern(raw_ident)`, and that is the whole of its definition:
/// this combinator is the general form of a thing the `ident` builtin used
/// to do in one hard-coded place (ADR 18).
///
/// The output only has to be `AsRef<str>`, so `intern(string)`,
/// `intern(until(";"))` and `intern(alpha1)` all work; interning something
/// that is not text is a type error at the call site.
///
/// Two properties the caller inherits, both from the interner and neither
/// from here: a symbol means nothing against a *different* interner (see
/// `rt::fold_pieces`, which builds one context per piece), and an
/// alternative that interns and then backtracks leaves its entry behind -
/// no symbol is ever wrong, the interner just holds more than the result
/// names.
pub fn intern<'a, S: Clone + std::fmt::Debug, O: AsRef<str>, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<crate::Symbol, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    move |input| {
        let text = p.parse_next(input)?;
        Ok(input.state.intern(text.as_ref()))
    }
}

/// `digit1`, `alpha1`, `multispace0`, ... - a run of a fixed ASCII class,
/// scanned eight bytes at a time instead of one character at a time (see
/// [`crate::ascii`]).
///
/// `min` is `0` or `1`: the difference between `digit0` and `digit1`, and the
/// only reason this can fail.
pub fn class<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S>>(
    class: AsciiClass,
    min: usize,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let n = class.run(input.as_bstr());
        if n < min {
            return Err(ErrMode::Backtrack(E::from_input(input)));
        }
        Ok(input.next_slice(n))
    }
}

/// `raw_ident` - [`class`] for the ASCII stretch, `wide` for a character that
/// is not ASCII.
///
/// An identifier is Unicode alphanumeric, so an umlaut belongs to the one it
/// stands in and the ASCII class alone would stop there. Deferred decoding is
/// exactly this split: the scan runs on bytes and a `char` is built only where
/// a byte with its high bit set says one is needed.
pub fn class_or_wide<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S>>(
    class: AsciiClass,
    wide: fn(char) -> bool,
    min: usize,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let rest = input.peek_slice(input.eof_offset());
        let n = class.run_or_wide(rest, wide);
        if n < min {
            return Err(ErrMode::Backtrack(E::from_input(input)));
        }
        Ok(input.next_slice(n))
    }
}

/// The implicit whitespace skip between the tokens of a syntactic rule.
///
/// Only the code generator calls this, and that is the point: what the skip
/// records is marked trivia, so a message can rank it below the token the
/// grammar was actually looking for. A `WS` that a grammar calls itself goes
/// through the ordinary path and is an ordinary rule.
#[inline]
pub fn skip_trivia<'a, S, E, F>(mut ws: F, input: &mut ParseInput<'a, S>) -> Result<(), ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    E: RtError<'a, S>,
    F: FnMut(&mut ParseInput<'a, S>) -> Result<(), ErrMode<E>>,
{
    let outer = input.state.in_trivia;
    input.state.in_trivia = true;
    let from = input.current_token_start();
    let r = ws(input);
    input.state.in_trivia = outer;
    // Where this skip ran, for `ParseContext::record`: an attempt that failed
    // inside the trivia its own rule skipped has not begun. Skips *chain* -
    // a rule skips at its start and the first element of its sequence skips
    // again at the same position - so one that begins where the last one
    // ended continues it rather than replacing it. A skip that begins
    // anywhere else starts a new chain, which is exactly the case where a
    // token was consumed in between. Only the diagnosing pass records
    // anything, so only it pays the two stores.
    if E::RECORDING {
        let to = input.current_token_start();
        let chain = &mut input.state.last_trivia;
        if chain.1 == from {
            chain.1 = to;
        } else {
            *chain = (from, to);
        }
    }
    r
}

/// Gives a builtin an expectation (`identifier`, `integer literal`) if it
/// failed without one - winnow's own primitives only report the position.
pub fn expected<'a, S: Clone + std::fmt::Debug, O, P, E: RtError<'a, S>>(
    what: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, ErrMode<E>>
where
    P: Parser<ParseInput<'a, S>, O, ErrMode<E>>,
{
    move |input| {
        let start = input.current_token_start();
        match p.parse_next(input) {
            Err(ErrMode::Backtrack(e)) => Err(ErrMode::Backtrack(e.expected(start, what))),
            r => r,
        }
    }
}

/// `fail("…")`: verbatim message, high priority - but not fatal. An error
/// that got further still wins (progress before priority).
pub fn fail<'a, S: Clone + std::fmt::Debug, O, E: RtError<'a, S>>(
    message: &'static str,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, ErrMode<E>> {
    move |input| Err(ErrMode::Backtrack(E::fail(input, message)))
}

/// Finishes a call of the public `parse_<rule>()`.
///
/// The returned error is not necessarily the most informative one - one that
/// got further may have been hidden along the way by a successful backtrack.
/// And if the rule succeeded without consuming everything, the recorded
/// reason is the answer - otherwise only "expected end of input" would remain.
pub fn finish<'a, S: Clone + std::fmt::Debug, O>(
    input: &mut ParseInput<'a, S>,
    result: Result<O, ErrMode<ParseError>>,
) -> Result<O, ParseError> {
    match result {
        Ok(v) => {
            if input.eof_offset() == 0 {
                Ok(v)
            } else {
                let e = ParseError::from_stream(input).add_expected("end of input");
                Err(input.state.best(e))
            }
        }
        Err(ErrMode::Backtrack(e) | ErrMode::Cut(e)) => Err(input.state.best(e)),
        Err(ErrMode::Incomplete(_)) => {
            Err(ParseError::from_stream(input).with_message("incomplete input"))
        }
    }
}

/// The fast pass of an entry point: `Some` only when it accepted the whole
/// input. Otherwise the input is back where it started - the diagnosing pass
/// may run - and the state is as the fast pass left it, `fold` included.
pub fn accepted<'a, S, O, F>(input: &mut ParseInput<'a, S>, fast: F) -> Option<O>
where
    S: Clone + std::fmt::Debug,
    F: FnOnce(&mut ParseInput<'a, S>) -> Result<O, ErrMode<EmptyError>>,
{
    let cp = input.checkpoint();
    match fast(input) {
        Ok(v) if input.eof_offset() == 0 => Some(v),
        _ => {
            input.reset(&cp);
            None
        }
    }
}

/// Runs a call of the public `parse_<rule>()` - ADR 17.
///
/// `fast` and `diagnose` are the rule's parser instantiated with `EmptyError`
/// and with [`ParseError`]; what happens between them is
/// [`Diagnose`]'s to say. A parse that succeeds costs the
/// fast pass alone. One that fails - or leaves input over, whose reason only
/// the diagnosing pass can name - is parsed again with the full engine, and
/// that error goes out through [`finish`].
pub fn entry<'a, S, O, F, D>(
    input: &mut ParseInput<'a, S>,
    fast: F,
    diagnose: D,
) -> Result<O, ParseError>
where
    S: Clone + std::fmt::Debug,
    F: FnOnce(&mut ParseInput<'a, S>) -> Result<O, ErrMode<EmptyError>>,
    D: FnOnce(&mut ParseInput<'a, S>) -> Result<O, ErrMode<ParseError>>,
{
    input.state.begin_parse();
    match input.state.diagnose {
        Diagnose::Eager => {}
        Diagnose::Off => return accepted(input, fast).ok_or_else(ParseError::undiagnosed),
        Diagnose::ReplayInPlace => {
            if let Some(v) = accepted(input, fast) {
                return Ok(v);
            }
        }
        Diagnose::Replay => {
            let snapshot = input.state.user_state.clone();
            if let Some(v) = accepted(input, fast) {
                return Ok(v);
            }
            input.state.user_state = snapshot;
        }
    }
    diagnose_from_here(input, diagnose)
}

/// [`entry`] for a `par_fold` rule: the replay starts at the item the fast
/// pass stopped in, not at the beginning of the input.
///
/// The fold in the rule's body ([`par_fold_recording`]) leaves behind where it
/// stopped and how many items it had accepted. Those items are independent of
/// one another and of any state - that is what `par_fold` promises (ADR 16),
/// and what lets pieces of the input be parsed on separate cores - so a
/// diagnosing pass that skips them sees exactly what a pass over everything
/// would see at that point: the same item, the same error, at the same
/// offset, numbered the same. What it does not see are the errors recorded
/// inside the accepted items, and those lie before this one and would lose
/// on progress anyway. The cost of a failure is one item in diagnose mode.
///
/// Should the tail parse after all - an item that did depend on what came
/// before it, which no checked `par_fold` has - the whole input is diagnosed
/// instead, as [`entry`] would.
pub fn entry_framed<'a, S, O, F, D>(
    input: &mut ParseInput<'a, S>,
    fast: F,
    diagnose: D,
) -> Result<O, ParseError>
where
    S: Clone + std::fmt::Debug,
    F: FnOnce(&mut ParseInput<'a, S>) -> Result<O, ErrMode<EmptyError>>,
    D: Fn(&mut ParseInput<'a, S>) -> Result<O, ErrMode<ParseError>>,
{
    input.state.begin_parse();
    let origin = input.current_token_start();
    let base = input.state.fold.base;
    match input.state.diagnose {
        Diagnose::Eager => return diagnose_from_here(input, diagnose),
        Diagnose::Off => return accepted(input, fast).ok_or_else(ParseError::undiagnosed),
        Diagnose::ReplayInPlace => {
            if let Some(v) = accepted(input, fast) {
                return Ok(v);
            }
        }
        Diagnose::Replay => {
            let snapshot = input.state.user_state.clone();
            if let Some(v) = accepted(input, fast) {
                return Ok(v);
            }
            input.state.user_state = snapshot;
        }
    }
    let cp = input.checkpoint();
    let FoldProgress { seen, at, .. } = input.state.fold;
    // Skip the items the fast pass accepted. `at` is absolute (a stream
    // position), `origin` is where this call began.
    let skip = at.saturating_sub(origin).min(input.eof_offset());
    input.next_slice(skip);
    input.state.fold = FoldProgress {
        base: base + seen,
        seen: 0,
        at,
    };
    match diagnose_from_here(input, &diagnose) {
        Err(e) => Err(e),
        Ok(_) => {
            input.reset(&cp);
            input.state.fold = FoldProgress {
                base,
                seen: 0,
                at: origin,
            };
            diagnose_from_here(input, diagnose)
        }
    }
}

/// The diagnosing pass from the current position: the recorded error
/// belongs to this run, and [`finish`] selects between it and the returned one.
fn diagnose_from_here<'a, S, O, D>(
    input: &mut ParseInput<'a, S>,
    diagnose: D,
) -> Result<O, ParseError>
where
    S: Clone + std::fmt::Debug,
    D: FnOnce(&mut ParseInput<'a, S>) -> Result<O, ErrMode<ParseError>>,
{
    input.state.furthest = None;
    let result = diagnose(input);
    finish(input, result)
}
