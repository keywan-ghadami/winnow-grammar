//! The search behind a scan for two or three single bytes (ADR 24 §12b).
//!
//! `until(";" | "\n")` looks for either byte. winnow's tuple `find_slice`
//! calls `memchr2`, which on x86-64 checks for AVX2 and builds its searcher
//! anew on every call - for a field of a dozen bytes, a large part of the
//! scan. The generated code keeps one of these in a `static` per scan site
//! instead: memchr's AVX2 searcher is built on first use and kept, and a
//! machine without AVX2 (or a target other than x86-64) gets `memchr2` /
//! `memchr3` as before. memchr's portable searcher and a plain byte loop were
//! measured as well, and both cost more than either.

#[cfg(target_arch = "x86_64")]
use memchr::arch::x86_64::avx2::memchr::{Three, Two};
#[cfg(target_arch = "x86_64")]
use std::sync::OnceLock;

/// A set of bytes to search a haystack for: the first position that holds
/// any of them.
pub trait FindBytes: Sync {
    /// The first position in `hay` whose byte is in the set.
    fn find(&self, hay: &[u8]) -> Option<usize>;
}

/// Two bytes, for a `static` the generated code builds with [`FindBytes2::new`].
#[derive(Debug)]
pub struct FindBytes2 {
    a: u8,
    b: u8,
    #[cfg(target_arch = "x86_64")]
    avx2: OnceLock<Option<Two>>,
}

impl FindBytes2 {
    /// The searcher for `a` and `b`; the AVX2 one is made on first use.
    pub const fn new(a: u8, b: u8) -> Self {
        FindBytes2 {
            a,
            b,
            #[cfg(target_arch = "x86_64")]
            avx2: OnceLock::new(),
        }
    }
}

impl FindBytes for FindBytes2 {
    #[inline]
    fn find(&self, hay: &[u8]) -> Option<usize> {
        #[cfg(target_arch = "x86_64")]
        if let Some(two) = self.avx2.get_or_init(|| Two::new(self.a, self.b)) {
            return two.find(hay);
        }
        memchr::memchr2(self.a, self.b, hay)
    }
}

/// Three bytes, as [`FindBytes2`].
#[derive(Debug)]
pub struct FindBytes3 {
    a: u8,
    b: u8,
    c: u8,
    #[cfg(target_arch = "x86_64")]
    avx2: OnceLock<Option<Three>>,
}

impl FindBytes3 {
    /// The searcher for `a`, `b` and `c`; the AVX2 one is made on first use.
    pub const fn new(a: u8, b: u8, c: u8) -> Self {
        FindBytes3 {
            a,
            b,
            c,
            #[cfg(target_arch = "x86_64")]
            avx2: OnceLock::new(),
        }
    }
}

impl FindBytes for FindBytes3 {
    #[inline]
    fn find(&self, hay: &[u8]) -> Option<usize> {
        #[cfg(target_arch = "x86_64")]
        if let Some(three) = self.avx2.get_or_init(|| Three::new(self.a, self.b, self.c)) {
            return three.find(hay);
        }
        memchr::memchr3(self.a, self.b, self.c, hay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Haystacks of every length up to past two AVX2 vectors, with the
    /// needles at the start, at the end, in between, and nowhere.
    fn haystacks() -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for len in 0..80usize {
            let base: Vec<u8> = (0..len).map(|i| b'a' + (i % 23) as u8).collect();
            out.push(base.clone());
            for at in [0, len / 2, len.saturating_sub(1)] {
                for needle in [b';', b'\n', b'\r', 0xc3] {
                    if at < len {
                        let mut h = base.clone();
                        h[at] = needle;
                        out.push(h.clone());
                        if at + 3 < len {
                            h[at + 3] = b'\n';
                            out.push(h);
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn two_finds_what_memchr2_finds() {
        static FIND: FindBytes2 = FindBytes2::new(b';', b'\n');
        for h in haystacks() {
            assert_eq!(FIND.find(&h), memchr::memchr2(b';', b'\n', &h), "{h:?}");
        }
    }

    #[test]
    fn three_finds_what_memchr3_finds() {
        static FIND: FindBytes3 = FindBytes3::new(b';', b'\n', b'\r');
        for h in haystacks() {
            assert_eq!(
                FIND.find(&h),
                memchr::memchr3(b';', b'\n', b'\r', &h),
                "{h:?}"
            );
        }
    }
}
