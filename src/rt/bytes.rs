//! The helpers of [`super`] that hand back what they matched, over
//! [`ParseBytes`] - the input of a grammar that declares `input bytes;`
//! (ADR 25).
//!
//! Everything that only steps, records and decides is shared with text
//! grammars and lives in [`super`]; what is here differs in what it yields.
//! Two rules decide it:
//!
//! * **What is matched by comparison is never checked** (D2): a literal, an
//!   ASCII class, a scan for a terminator compare bytes, and nothing they
//!   accept can be invalid UTF-8. A literal and a scan yield `&'a [u8]`; an
//!   ASCII class yields `&'a str`, proved by the class itself.
//! * **What becomes text is checked where it becomes text** (D3): [`text`],
//!   [`intern`] / [`intern_declared`] and the decoding built-ins validate the
//!   bytes they turn into `&str`, and a bad byte is a cut at its own offset
//!   (D5) - see [`invalid_utf8`].

use super::RtError;
use crate::ascii::AsciiClass;
use crate::error::Diagnostics;
use crate::ParseBytes;
use winnow::error::{ErrMode, StrContext};
use winnow::stream::{AsBStr, FindSlice, Stream};
use winnow::Parser;

/// Bytes an ASCII class accepted, as text - the one place the byte driver
/// turns bytes into text without checking them (ADR 25 D2).
///
/// # Safety
///
/// Every byte of `b` is below `0x80`. Each such byte is a complete one-byte
/// UTF-8 sequence, so a slice of them is valid UTF-8 - the argument of
/// ADR 24 §4.
#[inline]
unsafe fn proved_ascii_as_str(b: &[u8]) -> &str {
    debug_assert!(b.is_ascii());
    // SAFETY: the caller guarantees every byte is ASCII, see above.
    unsafe { std::str::from_utf8_unchecked(b) }
}

/// The prefix of `b` that `class` accepts, as text. Safe for any `b`: the
/// class is run here, and every class is a set of ASCII bytes.
#[doc(hidden)]
#[inline]
pub fn class_text(class: AsciiClass, b: &[u8]) -> &str {
    let n = class.run(b);
    // SAFETY: `AsciiClass::run` counts bytes of the class, and every class
    // (`src/ascii.rs`) holds ASCII bytes only.
    unsafe { proved_ascii_as_str(&b[..n]) }
}

/// Bytes the grammar matched by comparison but that this module did not scan
/// itself - a run of `digit`s, the text under `dec<T>(…)` - as text.
///
/// A safe function cannot take a caller's word for it that the bytes are
/// ASCII, so it looks: one `is_ascii`, a word at a time, which is all the
/// bytes of a digit run ever need. What is not ASCII is validated in full and
/// `Err` holds the offset of the first bad byte.
#[doc(hidden)]
#[inline]
pub fn ascii_or_checked(b: &[u8]) -> Result<&str, usize> {
    if b.is_ascii() {
        // SAFETY: just checked.
        return Ok(unsafe { proved_ascii_as_str(b) });
    }
    std::str::from_utf8(b).map_err(|e| e.valid_up_to())
}

/// A run of digits, taken: the bytes `p` consumed, as text by
/// [`ascii_or_checked`]. What a `digit{…}` run yields in a byte grammar,
/// where it is text as it is in a text grammar (D2).
#[doc(hidden)]
pub fn ascii_run<'a, S, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a str, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseBytes<'a, S>, &'a [u8], ErrMode<E>>,
    E: RtError<'a, S, &'a [u8]>,
{
    move |input| {
        let start = input.checkpoint();
        let run = p.parse_next(input)?;
        ascii_or_checked(run).map_err(|at| invalid_utf8(input, &start, at))
    }
}

/// The cut for a byte that is not UTF-8 where the grammar made text (D5).
///
/// `input` is moved to the bad byte, `at` bytes past `start`, so the error
/// sits at its absolute offset; the message names the offset and the byte:
/// `invalid UTF-8 at byte 1043: 0xff`. A cut and not a backtrack: no
/// alternative that reads the same bytes as text can succeed, and the error
/// has to name the byte, not the last alternative tried.
#[cold]
#[inline(never)]
pub fn invalid_utf8<'a, S: Clone + std::fmt::Debug, E: Diagnostics>(
    input: &mut ParseBytes<'a, S>,
    start: &<ParseBytes<'a, S> as Stream>::Checkpoint,
    at: usize,
) -> ErrMode<E> {
    input.reset(start);
    input.next_slice(at);
    let byte = input.as_bstr().first().copied().unwrap_or(0);
    ErrMode::Cut(E::invalid_utf8(input, byte))
}

/// The input not yet consumed, as bytes - [`super::rest`] for a byte grammar.
#[inline]
pub fn rest<'a, S: Clone + std::fmt::Debug>(input: &ParseBytes<'a, S>) -> &'a [u8] {
    let n = input.eof_offset();
    // SAFETY: `n` is the length of what is not yet consumed.
    unsafe { input.peek_slice_unchecked(n) }
}

/// Cut the first `n` bytes off the input without the bounds check - what
/// [`super`]'s unchecked cut is for a byte grammar, where every position is a
/// boundary and only the length matters (ADR 24 §12a).
///
/// # Safety
///
/// `n` is at most the length of [`rest`].
#[inline(always)]
unsafe fn take_in_bounds<'a, S: Clone + std::fmt::Debug>(
    input: &mut ParseBytes<'a, S>,
    n: usize,
) -> &'a [u8] {
    debug_assert!(n <= input.eof_offset());
    // SAFETY: the caller's guarantee, see above.
    unsafe { input.next_slice_unchecked(n) }
}

/// Consume `n` bytes that [`rest`] showed to be matched.
#[inline]
pub fn advance<S: Clone + std::fmt::Debug>(input: &mut ParseBytes<'_, S>, n: usize) {
    input.next_slice(n);
}

/// [`super::lit_byte`] over bytes: a literal of one ASCII byte, one compare
/// and one step in the fast pass, the literal with its expectation in the
/// diagnosing pass. Yields the byte, as a literal does in a byte grammar.
#[inline]
pub fn lit_byte<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S, &'a [u8]>>(
    byte: u8,
    lit: &'static str,
    char_lit: Option<char>,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a [u8], ErrMode<E>> {
    move |input| {
        if !E::RECORDING {
            let rest = rest(input);
            return if rest.first() == Some(&byte) {
                input.next_token();
                Ok(&rest[..1])
            } else {
                Err(ErrMode::Backtrack(E::from_input(input)))
            };
        }
        match char_lit {
            Some(c) => winnow::token::literal(c)
                .context(StrContext::Expected(
                    winnow::error::StrContextValue::CharLiteral(c),
                ))
                .parse_next(input),
            None => winnow::token::literal(lit)
                .context(StrContext::Expected(
                    winnow::error::StrContextValue::StringLiteral(lit),
                ))
                .parse_next(input),
        }
    }
}

/// [`super::scan_to_literal`] over bytes: what lay before the terminator,
/// unchecked - `until(t)` in a byte grammar yields `&'a [u8]` (D3).
pub fn scan_to_literal<'a, S: Clone + std::fmt::Debug, E>(
    needle: &'static str,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a [u8], ErrMode<E>> {
    move |input| {
        let end = match input.find_slice(needle) {
            Some(range) => range.start,
            None => input.eof_offset(),
        };
        // SAFETY: a scan's hit lies inside the input, and the end is its length.
        Ok(unsafe { take_in_bounds(input, end) })
    }
}

/// [`super::scan_to_line_ending`] over bytes.
pub fn scan_to_line_ending<'a, S: Clone + std::fmt::Debug, E>(
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a [u8], ErrMode<E>> {
    move |input| {
        let end = match input.find_slice(b'\n') {
            Some(range) => {
                let n = range.start;
                if input.peek_slice(n).ends_with(b"\r") {
                    n - 1
                } else {
                    n
                }
            }
            None => input.eof_offset(),
        };
        // SAFETY: a scan's hit lies inside the input, and the end is its length.
        Ok(unsafe { take_in_bounds(input, end) })
    }
}

/// [`super::scan_to_any`] over bytes.
pub fn scan_to_any<'a, S: Clone + std::fmt::Debug, E>(
    lits: &'static [&'static str],
    line_ending: bool,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a [u8], ErrMode<E>> {
    move |input| {
        let hit = match (lits, line_ending) {
            ([], false) => None,
            ([], true) => input.find_slice("\n"),
            ([a], false) => input.find_slice(*a),
            ([a], true) => input.find_slice((*a, "\n")),
            ([a, b], false) => input.find_slice((*a, *b)),
            ([a, b, c], false) => input.find_slice((*a, *b, *c)),
            ([a, b], true) => input.find_slice((*a, *b, "\n")),
            _ => unreachable!("the code generator limits a scan set to three needles"),
        };
        let end = match hit {
            Some(range) if !line_ending => range.start,
            Some(range) => {
                let n = range.start;
                let at_newline = input.peek_slice(range.end).ends_with(b"\n") && range.len() == 1;
                if at_newline && input.peek_slice(n).ends_with(b"\r") {
                    n - 1
                } else {
                    n
                }
            }
            None => input.eof_offset(),
        };
        // SAFETY: a scan's hit lies inside the input, and the end is its length.
        Ok(unsafe { take_in_bounds(input, end) })
    }
}

/// [`super::class`] over bytes: a run of an ASCII class, yielding `&'a str`
/// as it does in a text grammar - by construction, not by a check (D2).
pub fn class<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S, &'a [u8]>>(
    class: AsciiClass,
    min: usize,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let n = class.run(input.as_bstr());
        if n < min {
            return Err(ErrMode::Backtrack(E::from_input(input)));
        }
        // SAFETY: `n` bytes of an ASCII class, see `class_text`.
        Ok(unsafe { proved_ascii_as_str(input.next_slice(n)) })
    }
}

/// [`super::digit`] over bytes.
pub fn digit<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S, &'a [u8]>>(
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<char, ErrMode<E>> {
    move |input| match input.as_bstr().first() {
        Some(&b) if b.is_ascii_digit() => {
            input.next_token();
            Ok(b as char)
        }
        _ => Err(ErrMode::Backtrack(E::from_input(input))),
    }
}

/// One UTF-8 character at the start of `b`: `Ok(None)` at the end, `Err(0)`
/// where the sequence there is not valid (D3 - decoded with validation).
#[inline]
fn decode_char(b: &[u8]) -> Result<Option<char>, usize> {
    let Some(&first) = b.first() else {
        return Ok(None);
    };
    if first < 0x80 {
        return Ok(Some(first as char));
    }
    let width = match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return Err(0),
    };
    let seq = b.get(..width).unwrap_or(b);
    match std::str::from_utf8(seq) {
        Ok(s) if seq.len() == width => Ok(s.chars().next()),
        Ok(_) => Err(seq.len()),
        Err(e) => Err(e.valid_up_to()),
    }
}

/// [`super::class_or_wide`] over bytes - `raw_ident`, and so `ident`.
///
/// The ASCII part is scanned a word at a time as in a text grammar and not
/// checked; from a byte `>= 0x80` one character is decoded *with validation*
/// and tested with `wide`. A sequence that is not UTF-8 is the cut of
/// [`invalid_utf8`] at its first byte, wherever in the identifier it is.
pub fn class_or_wide<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S, &'a [u8]>>(
    class: AsciiClass,
    wide: fn(char) -> bool,
    min: usize,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a str, ErrMode<E>> {
    move |input| {
        let b = rest(input);
        let mut n = 0;
        let mut wide_seen = false;
        loop {
            n += class.run(&b[n..]);
            if n == b.len() || b[n] < 0x80 {
                break;
            }
            match decode_char(&b[n..]) {
                Ok(Some(c)) if wide(c) => {
                    n += c.len_utf8();
                    wide_seen = true;
                }
                Ok(_) => break,
                Err(bad) => {
                    let start = input.checkpoint();
                    return Err(invalid_utf8(input, &start, n + bad));
                }
            }
        }
        if n < min {
            return Err(ErrMode::Backtrack(E::from_input(input)));
        }
        let start = input.checkpoint();
        let run = input.next_slice(n);
        if !wide_seen {
            // SAFETY: `n` bytes of the ASCII class and nothing else.
            return Ok(unsafe { proved_ascii_as_str(run) });
        }
        // Every wide character was decoded with validation and the rest is
        // the class, so this check cannot fail; it is what an identifier
        // with a wide character pays for not writing `unsafe` beyond ASCII.
        std::str::from_utf8(run).map_err(|e| invalid_utf8(input, &start, e.valid_up_to()))
    }
}

/// One character other than the `except` bytes, decoded with validation -
/// what `char` reads between its quotes in a byte grammar. Backtracks at an
/// excluded byte and at the end, as `none_of` does; a sequence that is not
/// UTF-8 is the cut of [`invalid_utf8`].
pub fn char_except<'a, S: Clone + std::fmt::Debug, E: RtError<'a, S, &'a [u8]>>(
    except: &'static [u8],
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<char, ErrMode<E>> {
    move |input| {
        let b = rest(input);
        if b.first().is_some_and(|c| except.contains(c)) {
            return Err(ErrMode::Backtrack(E::from_input(input)));
        }
        match decode_char(b) {
            Ok(Some(c)) => {
                input.next_slice(c.len_utf8());
                Ok(c)
            }
            Ok(None) => Err(ErrMode::Backtrack(E::from_input(input))),
            Err(bad) => {
                let start = input.checkpoint();
                Err(invalid_utf8(input, &start, bad))
            }
        }
    }
}

/// `text(p)` in a byte grammar: run `p` and check the bytes it yields as
/// UTF-8 (D3). `p` is `take(…)` of the pattern for `text(…)` itself and the
/// delimited body for `string`; either way its bytes lie in the input, so a
/// bad byte is reported at its absolute offset.
pub fn text<'a, S, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<&'a str, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseBytes<'a, S>, &'a [u8], ErrMode<E>>,
    E: RtError<'a, S, &'a [u8]>,
{
    move |input| {
        let start = input.checkpoint();
        let whole = rest(input);
        let bytes = p.parse_next(input)?;
        match std::str::from_utf8(bytes) {
            Ok(t) => Ok(t),
            Err(e) => {
                let at = offset_in(whole, bytes);
                Err(invalid_utf8(input, &start, at + e.valid_up_to()))
            }
        }
    }
}

/// Where `part` starts in `whole`: the bytes a pattern yields lie in the
/// input it read, so this is the offset of the stretch that became text. A
/// value from elsewhere - a hand-written rule's own buffer - is reported at
/// the start of the pattern.
#[inline]
fn offset_in(whole: &[u8], part: &[u8]) -> usize {
    let at = (part.as_ptr() as usize).wrapping_sub(whole.as_ptr() as usize);
    if at <= whole.len() && at + part.len() <= whole.len() {
        at
    } else {
        0
    }
}

/// `dec<T>(p)` in a byte grammar: the bytes `p` matched, read as a `T`.
///
/// `p` yields a `&str` when it is an ASCII class run and the bytes it
/// consumed otherwise; those are digits, signs and points the grammar
/// compared, and [`ascii_or_checked`] turns them into text with one
/// `is_ascii`. What is not text is the cut of D5, what does not read as a
/// `T` the error of [`super::dec`].
pub fn dec<'a, S, T, P, O, E>(
    mut p: P,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<T, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
    O: AsRef<[u8]>,
    P: Parser<ParseBytes<'a, S>, O, ErrMode<E>>,
    E: RtError<'a, S, &'a [u8]>,
{
    move |input| {
        let cp = input.checkpoint();
        let matched = p.parse_next(input)?;
        let text = match ascii_or_checked(matched.as_ref()) {
            Ok(t) => t,
            Err(at) => return Err(invalid_utf8(input, &cp, at)),
        };
        match text.parse::<T>() {
            Ok(v) => Ok(v),
            Err(e) => {
                input.reset(&cp);
                Err(ErrMode::Backtrack(E::external(input, e)))
            }
        }
    }
}

/// `intern(p)` in a byte grammar, into the context's interner: the bytes `p`
/// yields, checked only when the interner does not already hold them (D4).
pub fn intern<'a, S, O, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<crate::Symbol, ErrMode<E>>
where
    S: Clone + std::fmt::Debug,
    O: AsRef<[u8]>,
    P: Parser<ParseBytes<'a, S>, O, ErrMode<E>>,
    E: RtError<'a, S, &'a [u8]>,
{
    move |input| {
        let start = input.checkpoint();
        let whole = rest(input);
        let value = p.parse_next(input)?;
        let bytes = value.as_ref();
        match input.state.intern_bytes(bytes) {
            Ok(sym) => Ok(sym),
            Err(bad) => {
                let at = offset_in(whole, bytes);
                Err(invalid_utf8(input, &start, at + bad))
            }
        }
    }
}

/// `intern(p)` in a byte grammar that declares `interner I;`: as [`intern`],
/// with the declared interner's [`lookup_bytes`](crate::Interner::lookup_bytes)
/// asked first.
pub fn intern_declared<'a, I, S, O, P, E>(
    mut p: P,
) -> impl FnMut(&mut ParseBytes<'a, S>) -> Result<crate::Symbol, ErrMode<E>>
where
    I: crate::Interner,
    S: Clone + std::fmt::Debug + crate::InternerOf<I>,
    O: AsRef<[u8]>,
    P: Parser<ParseBytes<'a, S>, O, ErrMode<E>>,
    E: RtError<'a, S, &'a [u8]>,
{
    move |input| {
        let start = input.checkpoint();
        let whole = rest(input);
        let value = p.parse_next(input)?;
        let bytes = value.as_ref();
        let interner = crate::InternerOf::<I>::interner(&mut input.state.user_state);
        if let Some(sym) = interner.lookup_bytes(bytes) {
            return Ok(sym);
        }
        match std::str::from_utf8(bytes) {
            Ok(t) => Ok(interner.intern(t)),
            Err(e) => {
                let at = offset_in(whole, bytes);
                Err(invalid_utf8(input, &start, at + e.valid_up_to()))
            }
        }
    }
}

/// [`super::frames`] for a byte grammar: [`super::frames_bytes`].
pub fn frames(input: &[u8], boundary: &str, n: usize) -> Vec<std::ops::Range<usize>> {
    super::frames_bytes(input, boundary.as_bytes(), n)
}
