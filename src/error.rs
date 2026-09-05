//! Der Fehlertyp der erzeugten Parser - die Diagnose-Engine.
//!
//! Vertrag: `docs/adr/adr15-diagnostics.md`. Die Auswahl zwischen
//! konkurrierenden Fehlern folgt derselben Rangfolge wie in syn-grammar
//! (ADR 13 dort): **Fortschritt, dann Prioritaet, dann Zusammenfassung**.
//! Im Text ist der Fortschritt ein Byte-Offset - `LocatingSlice` liefert ihn
//! umsonst, ein Cursor-Kunstgriff wie in syn-grammar ist nicht noetig.
//!
//! winnow reicht Fehler ueber `ParserError::or` durch `alt` - deshalb
//! genuegt es, die Auswahl dort zu implementieren, und jede Alternative
//! im erzeugten Code bekommt sie geschenkt. Was `alt` nicht sieht, sind
//! Fehler, die ein *erfolgreiches* Zuruecksetzen verwirft (`x?`, `x*`);
//! dafuer fuehrt [`crate::ParseContext`] die weiteste Fehlschlagstelle mit.

use std::fmt;
use winnow::error::{AddContext, FromExternalError, ParserError, StrContext};
use winnow::stream::{AsBStr, Location, Stream};

/// Gewoehnlicher Parsefehler.
pub const PRIO_NORMAL: u8 = 0;
/// Eine benannte Alternative (`# "…"`) ist an ihrer Grenze gescheitert.
pub const PRIO_LABELED: u8 = 10;
/// Zusammengefasste Erwartungen mehrerer Alternativen (`expected one of: …`).
pub const PRIO_AGGREGATED: u8 = 20;
/// `fail("…")`: schlaegt an derselben Stelle alles andere.
pub const PRIO_STRUCTURAL: u8 = 50;

/// Ein Parsefehler mit allem, was Auswahl und Anzeige brauchen.
///
/// Zeigergross: der Inhalt liegt in einem [`Kern`] auf dem Heap, die Felder
/// sind per `Deref` erreichbar (`e.expected`, `e.offset`). Ein Fehler ist der
/// seltene Pfad, der Erfolg der haeufige - und jede Closure-Ebene des
/// erzeugten Parsers haelt ein `Result<_, ErrMode<ParseError>>` auf dem
/// Stack. Mit dem Inhalt inline (rund 130 Byte) lief eine 500-fach
/// verschachtelte Regel im Debug-Build in den Stack-Overflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(Box<Kern>);

impl std::ops::Deref for ParseError {
    type Target = Kern;
    fn deref(&self) -> &Kern {
        &self.0
    }
}

impl std::ops::DerefMut for ParseError {
    fn deref_mut(&mut self) -> &mut Kern {
        &mut self.0
    }
}

/// Der Inhalt eines [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kern {
    /// Fuer die AUSWAHL: Byte-Offset, an dem es schiefging. Wer weiter kam,
    /// war naeher an der gemeinten Ableitung.
    pub offset: usize,
    /// Was an dieser Stelle erwartet wurde, in Anzeigeform (``"`;`"``,
    /// `"identifier"`, `"function argument"`). Dedupliziert.
    pub expected: Vec<String>,
    /// Eine wortwoertliche Meldung (`fail("…")`, externe Fehler wie
    /// "number too large"). Ersetzt die `expected`-Zeile.
    pub message: Option<String>,
    /// Was tatsaechlich dastand: das naechste Wort bzw. Zeichen. `None` am
    /// Ende der Eingabe.
    pub found: Option<String>,
    /// Die Regeln, in denen der Fehler auftrat, innerste zuerst. Nur Anzeige.
    pub rule_stack: Vec<String>,
    /// Rang bei GLEICHER Stelle. Siehe die `PRIO_*`-Konstanten.
    pub priority: u8,
}

impl ParseError {
    /// Fehler an der aktuellen Position des Stroms.
    pub fn from_stream<I: Stream + Location + AsBStr>(input: &I) -> Self {
        ParseError(Box::new(Kern {
            offset: input.current_token_start(),
            expected: Vec::new(),
            message: None,
            found: naechstes_wort(input.as_bstr()),
            rule_stack: Vec::new(),
            priority: PRIO_NORMAL,
        }))
    }

    /// Haengt eine Erwartung an, falls sie noch nicht dasteht.
    pub fn erwarte(mut self, was: impl Into<String>) -> Self {
        let was = was.into();
        if !self.expected.contains(&was) {
            self.expected.push(was);
        }
        self
    }

    /// Setzt eine wortwoertliche Meldung.
    pub fn mit_meldung(mut self, meldung: impl Into<String>) -> Self {
        self.message = Some(meldung.into());
        self
    }

    /// Setzt die Prioritaet.
    pub fn mit_prioritaet(mut self, prio: u8) -> Self {
        self.priority = prio;
        self
    }

    /// Haengt einen Regelnamen an den Stapel - auf dem Rueckgabepfad, wenn eine
    /// aeussere Regel den Fehler herausreicht. Direkte Wiederholungen werden
    /// verschluckt.
    pub fn push_rule(&mut self, rule: &str) {
        if self.rule_stack.last().map(String::as_str) != Some(rule) {
            self.rule_stack.push(rule.to_string());
        }
    }

    /// Waehlt aus zwei konkurrierenden Fehlern den aussagekraeftigeren.
    ///
    /// 1. **Fortschritt**: wer weiter im Input kam, gewinnt - auch gegen ein
    ///    `fail(..)`, das frueher stand.
    /// 2. **Prioritaet** bei gleicher Stelle: `fail` > Zusammenfassung > Label >
    ///    Standard.
    /// 3. Bei Gleichstand werden die Erwartungen **vereinigt**: aus zwei
    ///    Alternativen an derselben Stelle wird `expected one of: …`.
    pub fn merge(mut self, other: Self) -> Self {
        use std::cmp::Ordering::*;
        match self.offset.cmp(&other.offset) {
            Greater => return self,
            Less => return other,
            Equal => {}
        }
        match self.priority.cmp(&other.priority) {
            Greater => return self,
            Less => return other,
            Equal => {}
        }
        let other = *other.0;
        for e in other.expected {
            if !self.expected.contains(&e) {
                self.expected.push(e);
            }
        }
        if self.message.is_none() {
            self.message = other.message;
        }
        // Der spaetere Zweig bestimmt den Stapel - wie in syn-grammar gewinnt
        // bei Gleichstand der zuletzt gemerkte.
        if !other.rule_stack.is_empty() {
            self.rule_stack = other.rule_stack;
        }
        if self.expected.len() > 1 {
            self.priority = self.priority.max(PRIO_AGGREGATED);
        }
        self
    }

    /// Die erste Zeile der Meldung - ohne Position und Regelstapel.
    pub fn kopf(&self) -> String {
        if let Some(m) = &self.message {
            return m.clone();
        }
        let mut erwartet = self.expected.clone();
        erwartet.sort();
        erwartet.dedup();
        let erwartung = match erwartet.len() {
            0 => None,
            1 => Some(format!("expected {}", erwartet[0])),
            _ => Some(format!("expected one of: {}", erwartet.join(", "))),
        };
        match (&self.found, erwartung) {
            (None, Some(e)) => format!("unexpected end of input, {e}"),
            (None, None) => "unexpected end of input".to_string(),
            (Some(f), Some(e)) => format!("{e}; found unexpected token `{f}`"),
            (Some(f), None) => format!("unexpected token `{f}`"),
        }
    }

    /// Zeile und Spalte (1-basiert) von [`Kern::offset`] in `source`.
    pub fn zeile_spalte(&self, source: &str) -> (usize, usize) {
        let bis = self.offset.min(source.len());
        let vor = &source[..bis];
        let zeile = vor.matches('\n').count() + 1;
        let spalte = vor.rsplit('\n').next().map_or(0, |z| z.chars().count()) + 1;
        (zeile, spalte)
    }

    /// Die vollstaendige Meldung mit Position, wie sie ein Nutzer sehen soll.
    ///
    /// `Display` laesst die Position weg, weil winnows eigener `ParseError`
    /// (aus `Parser::parse`) sie samt Quellzeile voranstellt; wer ueber
    /// `parse_next` geht, hat die Quelle selbst und ruft dies hier.
    pub fn render(&self, source: &str) -> String {
        let (zeile, spalte) = self.zeile_spalte(source);
        let mut s = format!("{} at line {}, column {}", self.kopf(), zeile, spalte);
        for r in &self.rule_stack {
            s.push_str("\nin ");
            s.push_str(r);
        }
        s
    }
}

/// Das naechste Wort (Buchstaben, Ziffern, `_`) oder das naechste Zeichen.
fn naechstes_wort(rest: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&rest[..rest.len().min(64)]);
    let erstes = text.chars().next()?;
    if erstes.is_alphanumeric() || erstes == '_' {
        Some(
            text.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect(),
        )
    } else if erstes == '\n' {
        Some("newline".to_string())
    } else {
        Some(erstes.to_string())
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kopf())?;
        for r in &self.rule_stack {
            write!(f, "\nin {r}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

impl<I: Stream + Location + AsBStr> ParserError<I> for ParseError {
    type Inner = Self;

    fn from_input(input: &I) -> Self {
        Self::from_stream(input)
    }

    /// `alt` reicht die Fehler seiner Zweige hier durch - das ist die
    /// Fehlerauswahl fuer jede Alternative im erzeugten Code.
    fn or(self, other: Self) -> Self {
        self.merge(other)
    }

    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl<I: Stream + Location + AsBStr> AddContext<I, StrContext> for ParseError {
    fn add_context(
        mut self,
        _input: &I,
        _start: &<I as Stream>::Checkpoint,
        ctx: StrContext,
    ) -> Self {
        match ctx {
            StrContext::Label(name) => self.push_rule(name),
            StrContext::Expected(was) => self = self.erwarte(was.to_string()),
            _ => {}
        }
        self
    }
}

impl<I: Stream + Location + AsBStr, E: fmt::Display> FromExternalError<I, E> for ParseError {
    fn from_external_error(input: &I, e: E) -> Self {
        Self::from_stream(input).mit_meldung(e.to_string())
    }
}
