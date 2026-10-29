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
#[derive(Default)]
pub(crate) struct Buf {
    data: Vec<u8>,
    filled: usize,
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

    /// Drops the first `k` bytes.
    pub(crate) fn drop_front(&mut self, k: usize) {
        self.data.copy_within(k..self.filled, 0);
        self.filled -= k;
    }
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
/// `parse` gets the bytes alone; an error it returns is moved from the
/// block's offsets to the stream's. At the end the rest is parsed whether or
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
    P: FnMut(&[u8]) -> Result<T, ParseError>,
    M: Fn(T, T) -> T,
{
    let boundary = boundary.as_bytes();
    let block = reading.block.max(1);
    let mut buf = Buf::default();
    // The stream's offset of `buf[0]`.
    let mut base = 0usize;
    // No boundary starts in `buf` before this: what was searched already.
    let mut searched = 0usize;
    let mut acc: Option<T> = None;
    let mut eof = false;
    loop {
        let target = buf.len() + block;
        while buf.len() < target && !eof {
            let len = buf.len();
            match buf.read_from(&mut reader, target - len) {
                Ok(0) => eof = true,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(at(base + len, format_args!("read failed: {e}"))),
            }
        }
        let cut = if eof {
            buf.len()
        } else {
            match memchr::memmem::rfind(&buf.bytes()[searched..], boundary) {
                Some(i) => searched + i + boundary.len(),
                None => {
                    if buf.len() > reading.max_frame {
                        return Err(no_boundary(base, reading.max_frame));
                    }
                    // A boundary may begin in the last bytes and end in the
                    // next block.
                    searched = buf.len().saturating_sub(boundary.len().saturating_sub(1));
                    continue;
                }
            }
        };
        if buf.len() - cut > reading.max_frame {
            return Err(no_boundary(base + cut, reading.max_frame));
        }
        if cut > 0 || (eof && acc.is_none()) {
            let v = parse(&buf.bytes()[..cut]).map_err(|mut e| {
                if !e.is_undiagnosed() {
                    e.offset += base;
                }
                e
            })?;
            acc = Some(match acc {
                Some(a) => merge(a, v),
                None => v,
            });
        }
        if eof {
            return Ok(acc.expect("the last step parses when nothing was parsed before"));
        }
        buf.drop_front(cut);
        base += cut;
        searched = 0;
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
