//! Runtime helpers for the generated code - the places where the
//! diagnostics engine ([`crate::ParseError`]) steps in.
//!
//! Everything here works concretely on [`ParseInput`] because it needs the
//! state (`input.state`): that is where the furthest failure position lives,
//! which a successful backtrack would otherwise discard.

use crate::error::{ParseError, PRIO_LABELED, PRIO_STRUCTURAL};
use crate::ParseInput;
use winnow::error::ErrMode;
use winnow::stream::{Location, Stream};
use winnow::Parser;

type Fehler = ErrMode<ParseError>;

/// `x?` - at most once. A failed attempt is **recorded**, not thrown away:
/// if the rule later fails at a shallower position or input is left over, it
/// is the better message.
pub fn opt_merkend<'a, S: Clone + std::fmt::Debug, O, P>(
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Option<O>, Fehler>
where
    P: Parser<ParseInput<'a, S>, O, Fehler>,
{
    move |input| {
        let cp = input.checkpoint();
        match p.parse_next(input) {
            Ok(v) => Ok(Some(v)),
            Err(ErrMode::Backtrack(e)) => {
                input.state.merke(&e);
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
pub fn repeat_merkend<'a, S: Clone + std::fmt::Debug, O, P>(
    min: usize,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Vec<O>, Fehler>
where
    P: Parser<ParseInput<'a, S>, O, Fehler>,
{
    move |input| {
        let mut items = Vec::new();
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
                    items.push(v);
                }
                Err(ErrMode::Backtrack(mut e)) => {
                    e.push_rule(&format!("item {}", items.len() + 1));
                    if items.len() < min {
                        return Err(ErrMode::Backtrack(e));
                    }
                    input.state.merke(&e);
                    input.reset(&cp);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(items)
    }
}

/// A labelled alternative (`# "…"`). If it fails at its starting position,
/// its name counts as the expectation instead of the internal message:
/// ``expected `(` `` becomes `expected function argument`. If it made
/// progress, its own message is the more informative one and stays.
pub fn beschriftet<'a, S: Clone + std::fmt::Debug, O, P>(
    label: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, Fehler>
where
    P: Parser<ParseInput<'a, S>, O, Fehler>,
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
pub fn erwartet<'a, S: Clone + std::fmt::Debug, O, P>(
    was: &'static str,
    mut p: P,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, Fehler>
where
    P: Parser<ParseInput<'a, S>, O, Fehler>,
{
    move |input| {
        let start = input.current_token_start();
        match p.parse_next(input) {
            Err(ErrMode::Backtrack(e))
                if e.offset == start && e.expected.is_empty() && e.message.is_none() =>
            {
                Err(ErrMode::Backtrack(e.erwarte(was)))
            }
            r => r,
        }
    }
}

/// `fail("…")`: verbatim message, high priority - but not fatal. An error
/// that got further still wins (progress before priority).
pub fn fail<'a, S: Clone + std::fmt::Debug, O>(
    meldung: &'static str,
) -> impl FnMut(&mut ParseInput<'a, S>) -> Result<O, Fehler> {
    move |input| {
        Err(ErrMode::Backtrack(
            ParseError::from_stream(input)
                .mit_meldung(meldung)
                .mit_prioritaet(PRIO_STRUCTURAL),
        ))
    }
}

/// Finishes a call of the public `parse_<rule>()`.
///
/// The returned error is not necessarily the most informative one - one that
/// got further may have been hidden along the way by a successful backtrack.
/// And if the rule succeeded without consuming everything, the recorded
/// reason is the answer - otherwise only "expected end of input" would remain.
pub fn abschluss<'a, S: Clone + std::fmt::Debug, O>(
    input: &mut ParseInput<'a, S>,
    ergebnis: Result<O, Fehler>,
) -> Result<O, ParseError> {
    match ergebnis {
        Ok(v) => {
            if input.eof_offset() == 0 {
                Ok(v)
            } else {
                let e = ParseError::from_stream(input).erwarte("end of input");
                Err(input.state.beste(e))
            }
        }
        Err(ErrMode::Backtrack(e) | ErrMode::Cut(e)) => Err(input.state.beste(e)),
        Err(ErrMode::Incomplete(_)) => {
            Err(ParseError::from_stream(input).mit_meldung("incomplete input"))
        }
    }
}
