//! Laufzeithelfer fuer den erzeugten Code - die Stellen, an denen die
//! Diagnose-Engine ([`crate::ParseError`]) eingreift.
//!
//! Alles hier arbeitet konkret auf [`ParseInput`], weil es den Zustand
//! (`input.state`) braucht: dort liegt die weiteste Fehlschlagstelle, die ein
//! erfolgreiches Zuruecksetzen sonst verwerfen wuerde.

use crate::error::{ParseError, PRIO_LABELED, PRIO_STRUCTURAL};
use crate::ParseInput;
use winnow::error::ErrMode;
use winnow::stream::{Location, Stream};
use winnow::Parser;

type Fehler = ErrMode<ParseError>;

/// `x?` - hoechstens einmal. Ein gescheiterter Versuch wird **gemerkt**, nicht
/// weggeworfen: scheitert die Regel spaeter an flacherer Stelle oder bleibt
/// Eingabe uebrig, ist er die bessere Meldung.
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

/// `x*` / `x+` - Wiederholung mit Mindestanzahl. Der Grund, warum es nicht
/// weiterging, wird gemerkt und traegt den Index des versuchten Elements
/// (`in item 3`). Unter der Mindestanzahl ist er der Fehler selbst.
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
                    // Zero-Progress-Schutz: sonst dreht sich die Schleife ewig,
                    // wenn das Element ohne Verbrauch passt.
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

/// Eine benannte Alternative (`# "…"`). Scheitert sie an ihrer Anfangsstelle,
/// zaehlt ihr Name als Erwartung statt der internen Meldung: aus
/// ``expected `(` `` wird `expected function argument`. Kam sie voran, ist ihre
/// eigene Meldung die aussagekraeftigere und bleibt.
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

/// Gibt einem Builtin eine Erwartung (`identifier`, `integer literal`), falls
/// es ohne eine gescheitert ist - winnows eigene Primitiven melden nur die
/// Stelle.
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

/// `fail("…")`: Meldung wortwoertlich, hochprior - aber nicht fatal. Ein
/// weiter gekommener Fehler gewinnt trotzdem (Fortschritt vor Prioritaet).
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

/// Schliesst einen Aufruf der oeffentlichen `parse_<regel>()` ab.
///
/// Der zurueckgegebene Fehler ist nicht zwingend der aussagekraeftigste - ein
/// weiter gekommener kann unterwegs von einem erfolgreichen Zuruecksetzen
/// ueberdeckt worden sein. Und ist die Regel aufgegangen, ohne alles zu
/// verbrauchen, ist der gemerkte Grund die Antwort - sonst bliebe nur
/// "expected end of input".
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
