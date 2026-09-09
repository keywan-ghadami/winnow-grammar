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
use winnow::error::{AddContext, EmptyError, FromExternalError, ParserError, StrContext};
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
/// Pointer-sized: the content lives in a [`ErrorCore`] on the heap, the fields
/// are reachable via `Deref` (`e.expected`, `e.offset`). An error is the
/// rare path, success the common one - and every closure level of the
/// generated parser holds a `Result<_, ErrMode<ParseError>>` on the stack.
/// With the content inline (around 130 bytes), a 500-fold nested rule ran
/// into a stack overflow in the debug build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(Box<ErrorCore>);

impl std::ops::Deref for ParseError {
    type Target = ErrorCore;
    fn deref(&self) -> &ErrorCore {
        &self.0
    }
}

impl std::ops::DerefMut for ParseError {
    fn deref_mut(&mut self) -> &mut ErrorCore {
        &mut self.0
    }
}

/// The content of a [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCore {
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
    /// `true` only for [`ParseError::undiagnosed`]: the parse failed and
    /// nothing else is known - no position, no expectation.
    pub undiagnosed: bool,
    /// What would *also* have been accepted here, but is not required: the
    /// expectations of a repetition that had already met its minimum, and of
    /// an `x?`. See [`ParseError::merge`].
    pub also: Vec<String>,
    /// This error is an **optional continuation**: the grammar would have
    /// accepted more here and does not insist. Set by
    /// [`ParseContext::record`](crate::ParseContext::record), which is reached
    /// only from `opt_recording` and from a repetition at or above its
    /// minimum - below the minimum the element's error is returned instead,
    /// and a returned error is a requirement.
    pub optional: bool,
    /// Produced inside a `peek(…)` or `not(…)`. Never recorded: a lookahead
    /// consumes nothing and demands nothing, so what fails inside it is a test
    /// that said no rather than an expectation of the grammar at that
    /// position. See [`crate::rt::lookahead`].
    pub lookahead: bool,
    /// Recorded by the implicit whitespace skip. Ranks below every other
    /// optional continuation: the grammar was not looking for trivia here,
    /// it was looking for the next token.
    pub trivia: bool,
    /// Where the attempt that produced this error began, when it was recorded
    /// rather than returned.
    ///
    /// Two requirements at one offset are told apart by how long each has been
    /// open. `fn f() {` … `let x = 1` at end of input wants its `}`, and the
    /// alternative that read the `1` and hoped for a `(` after it wants
    /// something too - but it started two characters ago and the block started
    /// at the beginning of the file. The one that has been open longest is the
    /// structure the reader is actually inside.
    pub begun_at: Option<usize>,
}

impl ParseError {
    /// Error at the current position of the stream.
    pub fn from_stream<I: Stream + Location + AsBStr>(input: &I) -> Self {
        ParseError(Box::new(ErrorCore {
            offset: input.current_token_start(),
            expected: Vec::new(),
            message: None,
            found: next_word(input.as_bstr()),
            rule_stack: Vec::new(),
            priority: PRIO_NORMAL,
            undiagnosed: false,
            also: Vec::new(),
            optional: false,
            trivia: false,
            lookahead: false,
            begun_at: None,
        }))
    }

    /// The error of a fast pass that was not diagnosed
    /// ([`crate::Diagnose::Off`]): the parse failed, and that is all that is
    /// known. Carries no position; [`render`](Self::render) prints the
    /// message alone.
    pub fn undiagnosed() -> Self {
        ParseError(Box::new(ErrorCore {
            offset: 0,
            expected: Vec::new(),
            message: Some("parse failed; diagnostics are off".to_string()),
            found: None,
            rule_stack: Vec::new(),
            priority: PRIO_NORMAL,
            undiagnosed: true,
            also: Vec::new(),
            optional: false,
            trivia: false,
            lookahead: false,
            begun_at: None,
        }))
    }

    /// Whether this is an [`undiagnosed`](Self::undiagnosed) error.
    pub fn is_undiagnosed(&self) -> bool {
        self.undiagnosed
    }

    /// Appends an expectation unless it is already present.
    pub fn add_expected(mut self, what: impl Into<String>) -> Self {
        let what = what.into();
        if !self.expected.contains(&what) {
            self.expected.push(what);
        }
        self
    }

    /// Sets a verbatim message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Sets the priority.
    pub fn with_priority(mut self, prio: u8) -> Self {
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
        // A requirement outranks an optional continuation, whatever either
        // happens to expect. Nothing is dropped: what the optional side would
        // have accepted moves to `also`, for the note under the message.
        //
        // This is the axis that matters, and priority is not it. An `x?` or a
        // met-minimum repetition that expects two things was being promoted to
        // `PRIO_AGGREGATED` and so beat the single expectation the grammar
        // actually required at that position - which is how a missing `,`
        // came to be reported as `expected one of: "//", whitespace`.
        match (self.optional, other.optional) {
            (true, false) => return other.absorbing(*self.0),
            (false, true) => return self.absorbing(*other.0),
            _ => {}
        }
        // Then trivia, below every other optional continuation. The two are
        // separate axes and both are needed: at the top level of a grammar
        // whose entry rule is a repetition (`program = item*`), *everything*
        // is an optional continuation, so the first test cannot separate the
        // whitespace skip from the `}` the grammar was looking for.
        match (self.trivia, other.trivia) {
            (true, false) => return other.absorbing(*self.0),
            (false, true) => return self.absorbing(*other.0),
            _ => {}
        }
        // Then how long each has been open. Both are requirements at the same
        // position; the one whose attempt started earlier is the structure the
        // reader is inside, and the other is a guess made two characters ago.
        // Without this, `expected one of: `(`, `{`` - an identifier that could
        // have been a call or a struct literal - outranks the `}` an unclosed
        // block is missing, because two expectations beat one on priority.
        match (self.begun_at, other.begun_at) {
            (Some(a), Some(b)) if a < b => return self.absorbing_also(*other.0),
            (Some(a), Some(b)) if b < a => return other.absorbing_also(*self.0),
            _ => {}
        }
        match self.priority.cmp(&other.priority) {
            Greater => return self.absorbing_also(*other.0),
            Less => return other.absorbing_also(*self.0),
            Equal => {}
        }
        let other = *other.0;
        for e in other.also {
            if !self.also.contains(&e) {
                self.also.push(e);
            }
        }
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

    /// Takes over what `other` would have accepted, as a note rather than as
    /// an expectation. `self` is the requirement and keeps its own message,
    /// stack and priority.
    fn absorbing(mut self, other: ErrorCore) -> Self {
        for e in other.expected.into_iter().chain(other.also) {
            if !self.expected.contains(&e) && !self.also.contains(&e) {
                self.also.push(e);
            }
        }
        // The stack still follows the rule it always did - the most recently
        // recorded one wins - because where the parse *was* does not depend on
        // which of the two errors names the expectation. A repetition's `in
        // item 4` is the most specific thing anyone knows about this position
        // and is worth more than the enclosing rule's name.
        if !other.rule_stack.is_empty() {
            self.rule_stack = other.rule_stack;
        }
        // Its message is *not* adopted: `headline` prints a message instead of
        // the expectations, so taking one from an optional continuation would
        // hide the requirement - which is the whole of what this is fixing.
        self
    }

    /// [`absorbing`](Self::absorbing) for two errors of the same kind, where
    /// priority already chose which one speaks: the loser's expectations are
    /// still worth naming, as a note.
    ///
    /// Priority decides who holds the message, not what is forgotten. Without
    /// this, two optional continuations at one offset lose one of their
    /// expectations to the other's priority - which is how the `,` of a
    /// missing struct field vanished behind the whitespace skip.
    fn absorbing_also(mut self, other: ErrorCore) -> Self {
        for e in other.expected.into_iter().chain(other.also) {
            if !self.expected.contains(&e) && !self.also.contains(&e) {
                self.also.push(e);
            }
        }
        self
    }

    /// What would also have been accepted here, without the ones a reader
    /// cannot act on.
    ///
    /// Whitespace is dropped: the skip is greedy, so at this offset it has
    /// already taken everything there was, and supplying more only moves the
    /// same failure further along. "Expected whitespace" asks for an edit that
    /// cannot work.
    pub fn also_possible(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .also
            .iter()
            .filter(|e| *e != "whitespace")
            .filter(|e| !self.expected.contains(e))
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The first line of the message - without position and rule stack.
    pub fn headline(&self) -> String {
        if let Some(m) = &self.message {
            return m.clone();
        }
        // Nothing was required here, so what would have been accepted is all
        // there is to say - better than saying nothing.
        let mut expected = if self.expected.is_empty() {
            self.also_possible()
        } else {
            self.expected.clone()
        };
        expected.sort();
        expected.dedup();
        let expectation = match expected.len() {
            0 => None,
            1 => Some(format!("expected {}", expected[0])),
            _ => Some(format!("expected one of: {}", expected.join(", "))),
        };
        match (&self.found, expectation) {
            (None, Some(e)) => format!("unexpected end of input, {e}"),
            (None, None) => "unexpected end of input".to_string(),
            (Some(f), Some(e)) => format!("{e}; found unexpected token `{f}`"),
            (Some(f), None) => format!("unexpected token `{f}`"),
        }
    }

    /// Line and column (1-based) of [`ErrorCore::offset`] in `source`.
    ///
    /// The same function a grammar's own spans use -
    /// [`span::line_column`](crate::span::line_column).
    pub fn line_column(&self, source: &str) -> (usize, usize) {
        crate::span::line_column(source, self.offset)
    }

    /// The complete message with position, as a user should see it.
    ///
    /// `Display` leaves out the position because winnow's own `ParseError`
    /// (from `Parser::parse`) prepends it along with the source line; whoever
    /// goes through `parse_next` has the source themselves and calls this.
    pub fn render(&self, source: &str) -> String {
        if self.undiagnosed {
            return self.headline();
        }
        let (line, column) = self.line_column(source);
        let mut s = format!("{} at line {}, column {}", self.headline(), line, column);
        let also = self.also_possible();
        if !also.is_empty() {
            s.push_str(&format!("\nnote: also possible here: {}", also.join(", ")));
        }
        for r in &self.rule_stack {
            s.push_str("\nin ");
            s.push_str(r);
        }
        s
    }
}

/// The next word (letters, digits, `_`) or the next character.
fn next_word(rest: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&rest[..rest.len().min(64)]);
    let first = text.chars().next()?;
    if first.is_alphanumeric() || first == '_' {
        Some(
            text.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect(),
        )
    } else if first == '\n' {
        Some("newline".to_string())
    } else {
        Some(first.to_string())
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.headline())?;
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
            StrContext::Expected(what) => self = self.add_expected(what.to_string()),
            _ => {}
        }
        self
    }
}

impl<I: Stream + Location + AsBStr, E: fmt::Display> FromExternalError<I, E> for ParseError {
    fn from_external_error(input: &I, e: E) -> Self {
        Self::from_stream(input).with_message(e.to_string())
    }
}

/// What the runtime helpers need from an error type.
///
/// The generated parsers are generic over their error type, like winnow's
/// `Parser<I, O, E>`. Two types fill it: [`ParseError`] does the work - it is
/// the diagnostics engine of ADR 15 - and winnow's [`EmptyError`] does
/// nothing and costs nothing, which is the fast pass of ADR 17. Every method
/// here is a hook the runtime calls on the error path; the fast pass has no
/// error path worth the name, so its hooks are empty.
pub trait Diagnostics: Sized {
    /// Whether the context keeps the live rule stack and the furthest
    /// recorded error for this type. `false` skips both.
    const RECORDING: bool;

    /// `fail("…")` at the current position: a verbatim message that beats
    /// every other error at the same position - but not one that got further.
    fn fail<I: Stream + Location + AsBStr>(input: &I, message: &'static str) -> Self;

    /// The failed attempt at the `index`-th element of a repetition
    /// (1-based): `in item 3`.
    fn item(self, index: usize) -> Self;

    /// A labelled alternative (`# "…"`) that failed at `start`, its own
    /// position: the label is the expectation instead of the internal
    /// message. If it got further, its own message is the more informative
    /// one and stays.
    fn labelled(self, start: usize, label: &'static str) -> Self;

    /// A builtin that failed at `start` without an expectation gets one
    /// (`identifier`, `integer literal`) - winnow's own primitives only
    /// report the position.
    fn expected(self, start: usize, what: &'static str) -> Self;

    /// Remembers an error that a successful backtrack (`x?`, `x*`) is about
    /// to discard - see [`crate::ParseContext::record`]. `start` is where the
    /// discarded attempt began, which decides whether it was an optional
    /// continuation or an element that had already committed.
    fn record<S>(&self, ctx: &mut crate::ParseContext<S>, start: usize);

    /// Marks this error as one a lookahead produced - see
    /// [`crate::rt::lookahead`]. It still fails the alternative around it; it
    /// is only never *recorded*.
    fn in_lookahead(self) -> Self;

    /// [`record`](Self::record) for an alternative of a rule that lost, kept
    /// **only if it had begun**.
    ///
    /// `alt` throws away what the alternatives before the winning one found,
    /// and where a *shorter* alternative succeeds that can be the only error
    /// at the position the input actually goes wrong. An alternative that
    /// failed where it started has already told the `alt` above it everything
    /// it knows, and recording those would bury the message in every
    /// expectation of every branch not taken.
    fn record_if_begun<S>(&self, ctx: &mut crate::ParseContext<S>, start: usize);

    /// The error of a hand-written parser plugged into a grammar. Those
    /// return [`ParseError`] whatever the grammar's error type is.
    fn from_parse_error(e: ParseError) -> Self;

    /// This error as a [`ParseError`], when it carries one. `None` for the
    /// fast pass's empty type, which has nothing to give.
    ///
    /// Used where an error is *kept* rather than reported - `recover(…)`
    /// swallows one and records it in the context, and only the diagnosing
    /// pass has anything to record.
    fn into_parse_error(self) -> Option<ParseError>;

    /// An error from outside the parser - a `FromStr` that refused, say.
    /// Winnow's own `FromExternalError` cannot be required of `E` here
    /// without naming every foreign error type in the bound, so the hook
    /// lives on this trait instead.
    fn external<I: Stream + Location + AsBStr, X: fmt::Display>(input: &I, e: X) -> Self;
}

impl Diagnostics for ParseError {
    const RECORDING: bool = true;

    fn fail<I: Stream + Location + AsBStr>(input: &I, message: &'static str) -> Self {
        Self::from_stream(input)
            .with_message(message)
            .with_priority(PRIO_STRUCTURAL)
    }

    fn item(mut self, index: usize) -> Self {
        self.push_rule(&format!("item {index}"));
        self
    }

    fn labelled(mut self, start: usize, label: &'static str) -> Self {
        if self.offset == start {
            self.expected = vec![label.to_string()];
            self.message = None;
            self.rule_stack.clear();
            self.priority = self.priority.max(PRIO_LABELED);
        }
        self
    }

    fn expected(self, start: usize, what: &'static str) -> Self {
        if self.offset == start && self.expected.is_empty() && self.message.is_none() {
            self.add_expected(what)
        } else {
            self
        }
    }

    fn in_lookahead(mut self) -> Self {
        self.lookahead = true;
        self
    }

    fn record<S>(&self, ctx: &mut crate::ParseContext<S>, start: usize) {
        ctx.record(self, start);
    }

    fn record_if_begun<S>(&self, ctx: &mut crate::ParseContext<S>, start: usize) {
        if ctx.began(start, self.offset) {
            ctx.record(self, start);
        }
    }

    fn from_parse_error(e: ParseError) -> Self {
        e
    }

    fn into_parse_error(self) -> Option<ParseError> {
        Some(self)
    }

    fn external<I: Stream + Location + AsBStr, X: fmt::Display>(input: &I, e: X) -> Self {
        Self::from_stream(input).with_message(e.to_string())
    }
}

impl Diagnostics for EmptyError {
    const RECORDING: bool = false;

    fn fail<I: Stream + Location + AsBStr>(_input: &I, _message: &'static str) -> Self {
        EmptyError
    }

    fn item(self, _index: usize) -> Self {
        self
    }

    fn labelled(self, _start: usize, _label: &'static str) -> Self {
        self
    }

    fn expected(self, _start: usize, _what: &'static str) -> Self {
        self
    }

    fn in_lookahead(self) -> Self {
        self
    }

    fn record<S>(&self, _ctx: &mut crate::ParseContext<S>, _start: usize) {}

    fn record_if_begun<S>(&self, _ctx: &mut crate::ParseContext<S>, _start: usize) {}

    fn from_parse_error(_e: ParseError) -> Self {
        EmptyError
    }

    fn into_parse_error(self) -> Option<ParseError> {
        None
    }

    fn external<I: Stream + Location + AsBStr, X: fmt::Display>(_input: &I, _e: X) -> Self {
        EmptyError
    }
}
