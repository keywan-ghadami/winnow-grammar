//! A `par_fold` rule's input read from a [`Read`] - stage 0 of ADR 27.
//!
//! A block is read, cut just past the last frame boundary in it, and what lies
//! before the cut is parsed with the rule's own pieces driver; the rest waits
//! for the next block. A block starts and ends just past a boundary, so it is
//! a piece in the sense of ADR 24 §1, and the merged result is the sequential
//! parse's. Nothing about the generated parser changes: it never sees a
//! buffer edge.

use crate::error::{Diagnostics, ParseError};
use std::io::Read;
use winnow::stream::{LocatingSlice, Stream};

/// A read buffer: `data[..filled]` is what has been read. The rest of
/// `data` is zeroed once, when it grows, so that a read of a few bytes does
/// not clear a block's worth first.
///
/// It also keeps count of the lines it has dropped, so that an error can say
/// where it is when its input is gone ([`Buf::excerpt`]).
#[derive(Default)]
pub(crate) struct Buf {
    data: Vec<u8>,
    filled: usize,
    /// Bytes dropped so far: the stream's offset of `data[0]`.
    pub(crate) dropped: usize,
    /// `\n` among them.
    lines: usize,
    /// The stream's offset where the line `data[0]` is on begins.
    line_start: usize,
}

impl Buf {
    pub(crate) fn len(&self) -> usize {
        self.filled
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.data[..self.filled]
    }

    /// One read of up to `n` bytes, appended; `Ok(0)` is the end.
    pub(crate) fn read_from(&mut self, reader: &mut impl Read, n: usize) -> std::io::Result<usize> {
        let need = self.filled + n.max(1);
        if self.data.len() < need {
            self.data.resize(need.max(self.data.len() * 2), 0);
        }
        let end = (self.filled + n.max(1)).min(self.data.len());
        let got = reader.read(&mut self.data[self.filled..end])?;
        self.filled += got;
        Ok(got)
    }

    /// How much of the first `k` bytes may go while the line the `k`-th is
    /// on stays, for an error's excerpt: all of them, unless that line began
    /// within `limit` bytes before - a line longer than that is cut, so the
    /// bound on memory holds for one long line too.
    pub(crate) fn droppable(&self, k: usize, limit: usize) -> usize {
        match memchr::memrchr(b'\n', &self.data[..k]) {
            Some(i) if k - (i + 1) <= limit => i + 1,
            Some(_) => k,
            None if self.line_start == self.dropped && k <= limit => 0,
            None => k,
        }
    }

    /// Drops the first `k` bytes.
    pub(crate) fn drop_front(&mut self, k: usize) {
        let gone = &self.data[..k];
        self.lines += count_lines(gone);
        if let Some(i) = memchr::memrchr(b'\n', gone) {
            self.line_start = self.dropped + i + 1;
        }
        self.dropped += k;
        self.data.copy_within(k..self.filled, 0);
        self.filled -= k;
    }

    /// The line the stream position `offset` is on, if the buffer still
    /// holds it.
    pub(crate) fn excerpt(&self, offset: usize) -> Option<crate::error::Excerpt> {
        let at = offset.checked_sub(self.dropped)?;
        let bytes = self.bytes();
        if at > bytes.len() {
            return None;
        }
        let before = &bytes[..at];
        let line = self.lines + count_lines(before) + 1;
        let (start, whole) = match memchr::memrchr(b'\n', before) {
            Some(i) => (i + 1, true),
            None => (0, self.line_start == self.dropped),
        };
        let end = memchr::memchr(b'\n', &bytes[at..]).map_or(bytes.len(), |i| at + i);
        let mut line_bytes = &bytes[start..end];
        if let [rest @ .., b'\r'] = line_bytes {
            line_bytes = rest;
        }
        let text = String::from_utf8_lossy(line_bytes).into_owned();
        let prefix = String::from_utf8_lossy(&bytes[start..at.min(start + line_bytes.len())]);
        let mut pos = prefix.len().min(text.len());
        while !text.is_char_boundary(pos) {
            pos -= 1;
        }
        let column = if whole {
            text[..pos].chars().count() + 1
        } else {
            offset - self.line_start + 1
        };
        Some(crate::error::Excerpt {
            line,
            column,
            text,
            at: pos,
        })
    }
}

/// `\n` in `b`. Counted in `u8` over runs of 255 bytes, a shape the compiler
/// turns into a vector loop: `memchr_iter` pays per match, and a plain
/// `filter().count()` per byte - 3.5 instructions a byte, a fifth of a
/// 1BRC row's parse.
fn count_lines(b: &[u8]) -> usize {
    b.chunks(255)
        .map(|c| c.iter().map(|&x| (x == b'\n') as u8).sum::<u8>() as usize)
        .sum()
}

/// How `parse_<rule>_reader` reads: the size of a read, and the bound on what
/// may follow the last boundary.
///
/// The buffer holds at most `max_frame + block` bytes. An input in which more
/// than `max_frame` bytes follow the last boundary is an error at the offset
/// where they begin - not a buffer that grows with the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Bytes asked of the reader per step; `0` is read as `1`.
    pub block: usize,
    /// The longest stretch without a boundary that is waited out.
    pub max_frame: usize,
}

impl Default for Reading {
    /// Blocks of 1 MiB, frames of up to 16 MiB.
    fn default() -> Self {
        Reading {
            block: 1 << 20,
            max_frame: 1 << 24,
        }
    }
}

/// The driver behind `parse_<rule>_reader`: read `reader` in blocks, hand
/// each run of whole frames - the bytes up to just past the last `boundary` -
/// to `parse`, and fold the results with `merge`, in input order.
///
/// `parse` gets the bytes alone and answers with its value and the number of
/// items it accepted; an error it returns is moved from the block's offsets
/// and item numbers to the stream's. At the end the rest is parsed whether or
/// not it ends in a boundary, as the last piece of an input is; a rest left
/// empty is not parsed, unless the whole input was empty.
pub fn fold_reader<R, T, P, M>(
    mut reader: R,
    boundary: &str,
    reading: Reading,
    mut parse: P,
    merge: M,
) -> Result<T, ParseError>
where
    R: Read,
    P: FnMut(&[u8]) -> Result<(T, usize), ParseError>,
    M: Fn(T, T) -> T,
{
    let boundary = boundary.as_bytes();
    let block = reading.block.max(1);
    let mut buf = Buf::default();
    // Where what is not parsed yet begins in `buf`; before it, at most the
    // start of its line, kept for an error's excerpt.
    let mut start = 0usize;
    // No boundary starts in `buf[start..]` before this: what was searched.
    let mut searched = 0usize;
    let mut acc: Option<T> = None;
    // Items accepted in the blocks before, to number an error's from.
    let mut items = 0usize;
    let mut eof = false;
    loop {
        let target = buf.len() + block;
        while buf.len() < target && !eof {
            let len = buf.len();
            match buf.read_from(&mut reader, target - len) {
                Ok(0) => eof = true,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(at(buf.dropped + len, format_args!("read failed: {e}"))),
            }
        }
        let rest = buf.len() - start;
        let cut = if eof {
            rest
        } else {
            match memchr::memmem::rfind(&buf.bytes()[start + searched..], boundary) {
                Some(i) => searched + i + boundary.len(),
                None => {
                    if rest > reading.max_frame {
                        return Err(no_boundary(buf.dropped + start, reading.max_frame));
                    }
                    // A boundary may begin in the last bytes and end in the
                    // next block.
                    searched = rest.saturating_sub(boundary.len().saturating_sub(1));
                    continue;
                }
            }
        };
        if rest - cut > reading.max_frame {
            return Err(no_boundary(buf.dropped + start + cut, reading.max_frame));
        }
        if cut > 0 || (eof && acc.is_none()) {
            let origin = buf.dropped + start;
            let (v, n) = parse(&buf.bytes()[start..start + cut]).map_err(|mut e| {
                if !e.is_undiagnosed() {
                    e.offset += origin;
                    e.excerpt = buf.excerpt(e.offset);
                }
                super::renumber(e, items)
            })?;
            items += n;
            acc = Some(match acc {
                Some(a) => merge(a, v),
                None => v,
            });
        }
        if eof {
            return Ok(acc.expect("the last step parses when nothing was parsed before"));
        }
        start += cut;
        searched = 0;
        let gone = buf.droppable(start, reading.max_frame);
        buf.drop_front(gone);
        start -= gone;
    }
}

/// A block of a text grammar, checked as UTF-8 - the check `&str` input
/// implies, made where the bytes arrive. A bad byte is the error of ADR 25
/// D5, at its offset in the block.
pub fn text_block(block: &[u8]) -> Result<&str, ParseError> {
    std::str::from_utf8(block).map_err(|e| {
        let at = e.valid_up_to();
        let mut s = LocatingSlice::new(block);
        s.next_slice(at);
        ParseError::invalid_utf8(&s, block[at])
    })
}

pub(crate) fn at(offset: usize, message: impl std::fmt::Display) -> ParseError {
    let mut e = ParseError::external(&LocatingSlice::new(&b""[..]), message);
    e.offset = offset;
    e
}

fn no_boundary(offset: usize, max_frame: usize) -> ParseError {
    at(
        offset,
        format_args!("more than {max_frame} bytes without a frame boundary"),
    )
}
