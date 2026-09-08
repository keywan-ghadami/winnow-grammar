//! Turning a byte offset into a place a person can find - `TODO.md` §3.
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
}
