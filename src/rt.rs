//! Runtime helpers for the generated code - the places where the
//! diagnostics engine ([`crate::ParseError`]) steps in.
//!
//! Everything here works concretely on [`ParseInput`] because it needs the
//! state (`input.state`): that is where the furthest failure position lives,
//! which a successful backtrack would otherwise discard.

use crate::error::{ParseError, PRIO_LABELED, PRIO_STRUCTURAL};
use crate::ParseInput;
use winnow::error::ErrMode;
use winnow::stream::{FindSlice, Location, Stream};
use winnow::Parser;

type RtError = ErrMode<ParseError>;

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
pub fn scan_to_literal<'a, S: Clone + std::fmt::Debug>(
    needle: &'static str,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, RtError> {
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
pub fn scan_to_line_ending<'a, S: Clone + std::fmt::Debug>(
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<&'a str, RtError> {
    move |input| {
        let end = match input.find_slice('\n') {
            Some(range) => {
                let n = range.start;
                // `peek_slice` is a byte slice of the input up to the newline,
                // so its last byte is the character before it.
                if n > 0 && input.peek_slice(n).as_bytes()[n - 1] == b'\r' {
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

/// `x?` - at most once. A failed attempt is **recorded**, not thrown away:
/// if the rule later fails at a shallower position or input is left over, it
/// is the better message.
pub fn opt_recording<'a, S: Clone + std::fmt::Debug, O, P>(
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Option<O>, RtError>
where
    P: Parser<ParseInput<'a, S>, O, RtError>,
{
    move |input| {
        let cp = input.checkpoint();
        match p.parse_next(input) {
            Ok(v) => Ok(Some(v)),
            Err(ErrMode::Backtrack(e)) => {
                input.state.record(&e);
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
pub fn repeat_recording<'a, S: Clone + std::fmt::Debug, O, P>(
    min: usize,
    p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Vec<O>, RtError>
where
    P: Parser<ParseInput<'a, S>, O, RtError>,
{
    repeat_recording_bounded(min, None, p)
}

/// `x{n}` / `x{n,}` / `x{n,m}` - repetition with explicit bounds.
///
/// Greedy and possessive, like [`repeat_recording`]: it takes as many elements
/// as it can up to `max` and never gives one back to help a later pattern
/// match. Below `min` the element's own error is the failure; at `max` the
/// repetition simply stops, and whatever follows sees the rest of the input.
pub fn repeat_recording_bounded<'a, S: Clone + std::fmt::Debug, O, P>(
    min: usize,
    max: Option<usize>,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Vec<O>, RtError>
where
    P: Parser<ParseInput<'a, S>, O, RtError>,
{
    move |input| {
        let mut items = Vec::new();
        loop {
            if max.is_some_and(|m| items.len() >= m) {
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
                Err(ErrMode::Backtrack(mut e)) => {
                    e.push_rule(&format!("item {}", items.len() + 1));
                    if items.len() < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    input.state.record(&e);
                    input.reset(&cp);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(items)
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
pub fn fold_recording<'a, S, O, Acc, P, I, F>(
    min: usize,
    mut p: P,
    mut init: I,
    mut step: F,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Acc, RtError>
where
    S: Clone + std::fmt::Debug,
    P: Parser<ParseInput<'a, S>, O, RtError>,
    I: FnMut() -> Acc,
    F: FnMut(Acc, O) -> Acc,
{
    move |input| {
        let mut acc = init();
        let mut seen = 0usize;
        loop {
            let cp = input.checkpoint();
            let start = input.current_token_start();
            match p.parse_next(input) {
                Ok(v) => {
                    // Zero-progress guard: otherwise the loop spins forever
                    // when the element matches without consuming anything.
                    if input.current_token_start() == start {
                        input.reset(&cp);
                        break;
                    }
                    acc = step(acc, v);
                    seen += 1;
                }
                Err(ErrMode::Backtrack(mut e)) => {
                    e.push_rule(&format!("item {}", seen + 1));
                    if seen < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    input.state.record(&e);
                    input.reset(&cp);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(acc)
    }
}

/// A labelled alternative (`# "…"`). If it fails at its starting position,
/// its name counts as the expectation instead of the internal message:
/// ``expected `(` `` becomes `expected function argument`. If it made
/// progress, its own message is the more informative one and stays.
pub fn labelled<'a, S: Clone + std::fmt::Debug, O, P>(
    label: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, RtError>
where
    P: Parser<ParseInput<'a, S>, O, RtError>,
{
    move |input| {
        let start = input.current_token_start();
        match p.parse_next(input) {
            Err(ErrMode::Backtrack(mut e)) if e.offset == start => {
                e.expected = vec![label.to_string()];
                e.message = None;
                e.rule_stack.clear();
                e.priority = e.priority.max(PRIO_LABELED);
                Err(ErrMode::Backtrack(e))
            }
            r => r,
        }
    }
}

/// Gives a builtin an expectation (`identifier`, `integer literal`) if it
/// failed without one - winnow's own primitives only report the position.
pub fn expected<'a, S: Clone + std::fmt::Debug, O, P>(
    what: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, RtError>
where
    P: Parser<ParseInput<'a, S>, O, RtError>,
{
    move |input| {
        let start = input.current_token_start();
        match p.parse_next(input) {
            Err(ErrMode::Backtrack(e))
                if e.offset == start && e.expected.is_empty() && e.message.is_none() =>
            {
                Err(ErrMode::Backtrack(e.add_expected(what)))
            }
            r => r,
        }
    }
}

/// `fail("…")`: verbatim message, high priority - but not fatal. An error
/// that got further still wins (progress before priority).
pub fn fail<'a, S: Clone + std::fmt::Debug, O>(
    message: &'static str,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, RtError> {
    move |input| {
        Err(ErrMode::Backtrack(
            ParseError::from_stream(input)
                .with_message(message)
                .with_priority(PRIO_STRUCTURAL),
        ))
    }
}

/// Finishes a call of the public `parse_<rule>()`.
///
/// The returned error is not necessarily the most informative one - one that
/// got further may have been hidden along the way by a successful backtrack.
/// And if the rule succeeded without consuming everything, the recorded
/// reason is the answer - otherwise only "expected end of input" would remain.
pub fn finish<'a, S: Clone + std::fmt::Debug, O>(
    input: &mut ParseInput<'a, S>,
    result: Result<O, RtError>,
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
