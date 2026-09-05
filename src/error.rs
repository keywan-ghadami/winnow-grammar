//! The error type of the generated parsers - the diagnostics engine.
//!
//! Contract: `docs/adr/adr15-diagnostics.md`. The choice between competing
//! errors follows the same ranking as in syn-grammar (ADR 13 there):
//! **progress, then priority, then aggregation**. In text, progress is a
//! byte offset - `LocatingSlice` provides it for free, so a cursor trick
//! like the one in syn-grammar is not needed.
//!
//! winnow passes errors through `alt` via `ParserError::or` - so it is
//! enough to implement the selection there, and every alternative in the
//! generated code gets it for free. What `alt` does not see are errors
//! discarded by a *successful* backtrack (`x?`, `x*`); for those,
//! [`crate::ParseContext`] carries the furthest failure position along.

use std::fmt;
use winnow::error::{AddContext, FromExternalError, ParserError, StrContext};
use winnow::stream::{AsBStr, Location, Stream};

/// Ordinary parse error.
pub const PRIO_NORMAL: u8 = 0;
/// A labelled alternative (`# "…"`) failed at its boundary.
pub const PRIO_LABELED: u8 = 10;
/// Aggregated expectations of several alternatives (`expected one of: …`).
pub const PRIO_AGGREGATED: u8 = 20;
/// `fail("…")`: beats everything else at the same position.
pub const PRIO_STRUCTURAL: u8 = 50;

/// A parse error with everything that selection and display need.
///
/// Pointer-sized: the content lives in a [`Kern`] on the heap, the fields
/// are reachable via `Deref` (`e.expected`, `e.offset`). An error is the
/// rare path, success the common one - and every closure level of the
/// generated parser holds a `Result<_, ErrMode<ParseError>>` on the stack.
/// With the content inline (around 130 bytes), a 500-fold nested rule ran
/// into a stack overflow in the debug build.
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

/// The content of a [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kern {
    /// For SELECTION: byte offset at which things went wrong. Whoever got
    /// further was closer to the intended derivation.
    pub offset: usize,
    /// What was expected at this position, in display form (``"`;`"``,
    /// `"identifier"`, `"function argument"`). Deduplicated.
    pub expected: Vec<String>,
    /// A verbatim message (`fail("…")`, external errors such as
    /// "number too large"). Replaces the `expected` line.
    pub message: Option<String>,
    /// What was actually there: the next word or character. `None` at the
    /// end of input.
    pub found: Option<String>,
    /// The rules in which the error occurred, innermost first. Display only.
    pub rule_stack: Vec<String>,
    /// Rank at the SAME position. See the `PRIO_*` constants.
    pub priority: u8,
}

impl ParseError {
    /// Error at the current position of the stream.
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

    /// Appends an expectation unless it is already present.
    pub fn erwarte(mut self, was: impl Into<String>) -> Self {
        let was = was.into();
        if !self.expected.contains(&was) {
            self.expected.push(was);
        }
        self
    }

    /// Sets a verbatim message.
    pub fn mit_meldung(mut self, meldung: impl Into<String>) -> Self {
        self.message = Some(meldung.into());
        self
    }

    /// Sets the priority.
    pub fn mit_prioritaet(mut self, prio: u8) -> Self {
        self.priority = prio;
        self
    }

    /// Pushes a rule name onto the stack - on the return path, when an outer
    /// rule passes the error on. Immediate repetitions are swallowed.
    pub fn push_rule(&mut self, rule: &str) {
        if self.rule_stack.last().map(String::as_str) != Some(rule) {
            self.rule_stack.push(rule.to_string());
        }
    }

    /// Chooses the more informative of two competing errors.
    ///
    /// 1. **Progress**: whoever got further in the input wins - even against a
    ///    `fail(..)` that came earlier.
    /// 2. **Priority** at the same position: `fail` > aggregation > label >
    ///    default.
    /// 3. On a tie, the expectations are **merged**: two alternatives at the
    ///    same position become `expected one of: …`.
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
        // The later branch determines the stack - as in syn-grammar, on a tie
        // the most recently recorded one wins.
        if !other.rule_stack.is_empty() {
            self.rule_stack = other.rule_stack;
        }
        if self.expected.len() > 1 {
            self.priority = self.priority.max(PRIO_AGGREGATED);
        }
        self
    }

    /// The first line of the message - without position and rule stack.
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

    /// Line and column (1-based) of [`Kern::offset`] in `source`.
    pub fn zeile_spalte(&self, source: &str) -> (usize, usize) {
        let bis = self.offset.min(source.len());
        let vor = &source[..bis];
        let zeile = vor.matches('\n').count() + 1;
        let spalte = vor.rsplit('\n').next().map_or(0, |z| z.chars().count()) + 1;
        (zeile, spalte)
    }

    /// The complete message with position, as a user should see it.
    ///
    /// `Display` leaves out the position because winnow's own `ParseError`
    /// (from `Parser::parse`) prepends it along with the source line; whoever
    /// goes through `parse_next` has the source themselves and calls this.
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

/// The next word (letters, digits, `_`) or the next character.
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

    /// `alt` passes the errors of its branches through here - this is the
    /// error selection for every alternative in the generated code.
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
