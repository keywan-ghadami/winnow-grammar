//! Turning a byte offset into a place a person can find.
//!
//! A `@` binding yields a `Range<usize>` of byte offsets, and that is what it
//! should yield: it is what `LocatingSlice` knows, it costs nothing to produce,
//! and it slices the source directly. Line and column are a different thing -
//! they cannot be computed without the source, and computing them *during* the
//! parse would mean scanning back for newlines once per span, which is work
//! done for spans nobody looks at.
//!
//! So they are a presentation step, and this is where it lives. The error
//! engine has always done it for its own offsets; the same function is now
//! available for any offset a grammar produced.

/// Line and column (both 1-based) of `offset` in `source`.
///
/// The column counts **characters**, not bytes, so it is the column a person
/// sees. An offset past the end of `source` is clamped to the end.
///
/// ```
/// use winnow_grammar::span::line_column;
///
/// let src = "let x = 1;\nlet yy = 2;\n";
/// assert_eq!(line_column(src, 0), (1, 1));
/// assert_eq!(line_column(src, 4), (1, 5));   // `x`
/// assert_eq!(line_column(src, 15), (2, 5));  // `yy`
/// ```
pub fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let end = offset.min(source.len());
    let before = &source[..end];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |z| z.chars().count()) + 1;
    (line, column)
}

/// What a `@` span points at, and where it is.
pub trait SpanExt {
    /// The text the span covers.
    fn text<'a>(&self, source: &'a str) -> &'a str;

    /// Line and column (1-based) of the span's start and of its end - the
    /// pair an editor or a diagnostic needs.
    ///
    /// ```
    /// use winnow_grammar::span::SpanExt;
    ///
    /// let src = "alpha\nbeta gamma\n";
    /// let span = 11..16; // `gamma`
    /// assert_eq!(span.text(src), "gamma");
    /// assert_eq!(span.line_columns(src), ((2, 6), (2, 11)));
    /// ```
    fn line_columns(&self, source: &str) -> ((usize, usize), (usize, usize));

    /// The span's line with a caret under the part it covers - what a
    /// diagnostic about this span should print beneath its message.
    ///
    /// A span covering more than one line gets a caret to the end of the
    /// first, because that is where the reader has to look.
    ///
    /// ```
    /// use winnow_grammar::span::SpanExt;
    ///
    /// let src = "alpha\nbeta gamma\n";
    /// assert_eq!((11..16).caret(src), "   2 | beta gamma\n            ^^^^^");
    /// ```
    fn caret(&self, source: &str) -> String;
}

impl SpanExt for std::ops::Range<usize> {
    fn text<'a>(&self, source: &'a str) -> &'a str {
        let start = self.start.min(source.len());
        let end = self.end.clamp(start, source.len());
        &source[start..end]
    }

    fn line_columns(&self, source: &str) -> ((usize, usize), (usize, usize)) {
        (
            line_column(source, self.start),
            line_column(source, self.end),
        )
    }

    fn caret(&self, source: &str) -> String {
        caret(source, self.start, self.text(source).chars().count())
    }
}

/// How much of a long line is shown around the caret, in characters.
///
/// A minified JSON document is one line; a caret four thousand columns to the
/// right of a terminal is not a diagnostic. Past this width the line is
/// windowed around the position and the cut ends are marked with `…`.
const WINDOW: usize = 96;

/// The source line `offset` is on, with a caret under it - the two lines a
/// compiler prints beneath its message.
///
/// `width` is how many characters the caret should span; it is clamped to what
/// is left of the line, and to at least one. The returned text has **no**
/// trailing newline, so it can be joined into a message.
///
/// The caret is aligned by copying the line's own leading characters and
/// replacing everything but a tab with a space, so a tab-indented line points
/// at the right character in a terminal that renders tabs as anything at all.
///
/// ```
/// use winnow_grammar::span::caret;
///
/// let src = "struct S {\n    id: i32\n    temp: f64,\n}\n";
/// assert_eq!(
///     caret(src, 27, 4),
///     "   3 |     temp: f64,\n           ^^^^",
/// );
/// ```
pub fn caret(source: &str, offset: usize, width: usize) -> String {
    let offset = offset.min(source.len());
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    let text = source[start..end].trim_end_matches('\r');
    let (line, _) = line_column(source, offset);

    // Both counted in characters, because that is what a reader counts.
    let column = source[start..offset.min(start + text.len())]
        .chars()
        .count();
    let len = text.chars().count();

    let (from, to) = if len <= WINDOW {
        (0, len)
    } else {
        // Centre the caret, but never scroll past either end of the line.
        let from = column.saturating_sub(WINDOW / 2).min(len - WINDOW);
        (from, from + WINDOW)
    };
    let shown: String = text.chars().skip(from).take(to - from).collect();

    let gutter = format!("{line:>4} | ");
    let mut out = String::with_capacity(gutter.len() * 2 + shown.len() * 2 + 8);
    out.push_str(&gutter);
    if from > 0 {
        out.push('…');
    }
    out.push_str(&shown);
    if to < len {
        out.push('…');
    }

    out.push('\n');
    out.push_str(&" ".repeat(gutter.chars().count()));
    if from > 0 {
        out.push(' ');
    }
    for c in shown.chars().take(column.saturating_sub(from)) {
        out.push(if c == '\t' { '\t' } else { ' ' });
    }
    let width = width.clamp(1, to.saturating_sub(column).max(1));
    for _ in 0..width {
        out.push('^');
    }
    out
}
