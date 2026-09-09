//! Scanning a run of ASCII bytes eight at a time - the 1BRC trick, on a
//! `&str` input.
//!
//! A `&str` is valid UTF-8, and every byte of a multi-byte character has its
//! high bit set. An ASCII class therefore **cannot match a continuation
//! byte**, so the position where such a scan stops is always a character
//! boundary and the slice it cuts is valid UTF-8 by construction - the same
//! argument ADR 16 §4 makes for cutting frames.
//!
//! That is what lets the scan work on bytes and leave decoding to whoever
//! actually needs a `char`. Measured on `benches/deferred.rs`, a run of digits
//! against winnow's character-predicate `take_while`, through this table:
//!
//! | run | by char | by word |
//! |---|---|---|
//! | 2 bytes | 380 MiB/s | 438 MiB/s |
//! | 7 bytes | 746 MiB/s | 1.22 GiB/s |
//! | 20 bytes | 1.08 GiB/s | 2.72 GiB/s |
//! | 200 000 bytes | 1.30 GiB/s | 10.7 GiB/s |
//!
//! It is never slower, and it grows with the run - which is the shape a data
//! format has. End to end on an identifier-heavy grammar, where the runs are
//! short and the scan is a fraction of the work, it is ~10%
//! (`benches/interning.rs`, `parse/idents`).
//!
//! **A run of nothing is answered before the word loop is entered.** That
//! comparison is against a character predicate over runs that exist; the case
//! the scan was losing was the one where there is nothing to scan, and the
//! implicit whitespace skip makes that the most frequent call in any syntactic
//! grammar. Testing the first byte costs 6 instructions where the run is real
//! and saves 22 where it is not - worth 17% of Nikaia's compiler parsing its
//! own examples. `TODO.md` §5 has the measurement.
//!
//! Note what the middle of that comparison rules out: scanning the same class
//! one *byte* at a time reaches 2.67 GiB/s, so decoding was never the cost -
//! winnow already walks a `&str` byte by byte for an ASCII predicate. The
//! order of magnitude comes from testing eight bytes in one comparison, and
//! that is what needs the input as bytes. See
//! `docs/adr/adr23-deferred-decoding.md`.

const HIGH: u64 = 0x8080_8080_8080_8080;

#[inline]
const fn splat(b: u8) -> u64 {
    (b as u64) * 0x0101_0101_0101_0101
}

/// A byte class as up to three inclusive ASCII ranges, plus single bytes.
///
/// Everything the built-ins need is a handful of ranges, and a range is what a
/// word-at-a-time test can express: `lo <= b <= hi` for all eight bytes with
/// two additions and a mask.
#[derive(Clone, Copy)]
pub struct AsciiClass {
    ranges: &'static [(u8, u8)],
}

impl AsciiClass {
    pub const DIGIT: Self = Self {
        ranges: &[(b'0', b'9')],
    };
    pub const HEX_DIGIT: Self = Self {
        ranges: &[(b'0', b'9'), (b'a', b'f'), (b'A', b'F')],
    };
    pub const OCT_DIGIT: Self = Self {
        ranges: &[(b'0', b'7')],
    };
    pub const BINARY_DIGIT: Self = Self {
        ranges: &[(b'0', b'1')],
    };
    pub const ALPHA: Self = Self {
        ranges: &[(b'a', b'z'), (b'A', b'Z')],
    };
    /// What `raw_ident` matches in its ASCII half; the rest is decoded, see
    /// [`Self::run_or_wide`].
    pub const IDENT: Self = Self {
        ranges: &[(b'0', b'9'), (b'a', b'z'), (b'A', b'Z'), (b'_', b'_')],
    };
    pub const SPACE: Self = Self {
        ranges: &[(b'\t', b'\t'), (b' ', b' ')],
    };
    pub const MULTISPACE: Self = Self {
        ranges: &[(b'\t', b'\r'), (b' ', b' ')],
    };

    /// Whether one byte is in the class. The definition the word test below is
    /// checked against, exhaustively.
    #[inline]
    pub fn contains(self, b: u8) -> bool {
        let mut i = 0;
        while i < self.ranges.len() {
            let (lo, hi) = self.ranges[i];
            if b >= lo && b <= hi {
                return true;
            }
            i += 1;
        }
        false
    }

    /// For each byte of `w`, the high bit set if it is **not** in the class.
    ///
    /// The comparison has to be per byte, and a plain `sub` is not: a borrow
    /// out of one byte lands in the next and answers for it. `hasless` and
    /// `hasmore` of Bit Twiddling Hacks are *contains* tests for a whole word
    /// and do not survive being read byte by byte - which is what the
    /// exhaustive test in `tests/ascii_scan_test.rs` caught and an
    /// example-based one would not have.
    ///
    /// So the borrow is stopped instead of guarded against. Clearing the high
    /// bit of every byte leaves values below `0x80`; setting it again as a
    /// guard makes each byte at least `0x80`, so subtracting any `n <= 0x80`
    /// can never borrow across the boundary, and the guard bit survives
    /// exactly when the byte was `>= n`. A range is then `>= lo` and not
    /// `>= hi + 1`.
    ///
    /// A byte that had its high bit set to begin with - the lead or a
    /// continuation of a multi-byte character - is in no ASCII class, which is
    /// what masking the result with `!w` says. In the class means in *any*
    /// range, so the per-range masks are `or`ed and the answer inverted.
    #[inline]
    fn out_mask(self, w: u64) -> u64 {
        let guarded = (w & !HIGH) | HIGH;
        let mut inside = 0;
        let mut i = 0;
        while i < self.ranges.len() {
            let (lo, hi) = self.ranges[i];
            let ge_lo = guarded.wrapping_sub(splat(lo)) & HIGH;
            let ge_next = guarded.wrapping_sub(splat(hi + 1)) & HIGH;
            inside |= ge_lo & !ge_next;
            i += 1;
        }
        !(inside & !w) & HIGH
    }

    /// How many leading bytes of `b` are in the class.
    #[inline]
    pub fn run(self, b: &[u8]) -> usize {
        // A run of nothing is the common case, and one byte test answers it.
        //
        // The word loop pays its whole setup - eight bytes read, a mask built,
        // a count of trailing zeros divided - to report that the very first
        // byte is not in the class. Measured with callgrind, that is 43
        // instructions against 21 for the guarded path, and the guard costs 6
        // on a run that does have something in it.
        //
        // It is worth it because of *who* calls this. The implicit whitespace
        // skip runs between every pair of elements of every syntactic rule, and
        // in a language written without gratuitous blanks most of those find
        // nothing - so a parse pays at least one empty run per token, against
        // one non-empty run per token that is a class. Nikaia's compiler
        // parsing 2 000 small functions goes 281.6 M instructions -> 233.7 M,
        // which is **17%**; its 1BRC example over 200 000 rows goes 123.6 M ->
        // 120.0 M, and the per-byte scan it is being compared against is
        // 119.4 M, so the guard recovers seven eighths of what the word scan
        // costs there. See `TODO.md` §5 for the measurement this replaced.
        if b.is_empty() || !self.contains(b[0]) {
            return 0;
        }
        let mut n = 0;
        while n + 8 <= b.len() {
            let w = u64::from_le_bytes(b[n..n + 8].try_into().expect("eight bytes"));
            let out = self.out_mask(w);
            if out != 0 {
                return n + (out.trailing_zeros() as usize / 8);
            }
            n += 8;
        }
        while n < b.len() && self.contains(b[n]) {
            n += 1;
        }
        n
    }

    /// [`run`](Self::run), continuing through characters that are not ASCII
    /// when `wide` accepts them - which is where the decoding that was
    /// deferred actually happens, and only there.
    ///
    /// `raw_ident` needs this: its class is Unicode alphanumeric, so `über` is
    /// one identifier. The ASCII run is scanned a word at a time and a byte
    /// with its high bit set is the one place a `char` is built.
    #[inline]
    pub fn run_or_wide(self, s: &str, wide: fn(char) -> bool) -> usize {
        let b = s.as_bytes();
        let mut n = 0;
        loop {
            n += self.run(&b[n..]);
            if n == b.len() || b[n] < 0x80 {
                return n;
            }
            // Only now is anything decoded, and only this one character.
            let c = s[n..].chars().next().expect("a boundary, so a character");
            if !wide(c) {
                return n;
            }
            n += c.len_utf8();
        }
    }
}
