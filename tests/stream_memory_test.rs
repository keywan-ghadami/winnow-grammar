//! The promise of ADR 27: memory stays bounded however long the input is.
//!
//! A counting allocator records the peak of live heap bytes while a
//! generated input of 16 MiB is parsed from a `Read` - by a `par_fold`
//! rule's `parse_<rule>_reader` (stage 0) and by a `#[stream]` rule's. The
//! peak must stay within a small multiple of `max_frame + block`, and must
//! not grow with the input: the same parse over a quarter of the input peaks
//! at the same height.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use winnow_grammar::rt::{Parallelism, Reading};
use winnow_grammar::{grammar, ParseContext};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// `name;12\n` rows, made as they are read, until about `bytes` are out.
struct Rows {
    left: usize,
    row: Vec<u8>,
    at: usize,
    n: u64,
}

impl Rows {
    fn new(bytes: usize) -> Self {
        Rows {
            left: bytes,
            row: Vec::new(),
            at: 0,
            n: 0,
        }
    }
}

impl Read for Rows {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at == self.row.len() {
            // Whole rows only: the last one may end past `left`.
            if self.left == 0 {
                return Ok(0);
            }
            self.n += 1;
            self.row.clear();
            self.row
                .extend(format!("station{};{}\n", self.n % 97, self.n % 1000).as_bytes());
            self.at = 0;
            self.left = self.left.saturating_sub(self.row.len());
        }
        let n = buf.len().min(self.row.len() - self.at);
        buf[..n].copy_from_slice(&self.row[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

grammar! {
    grammar Rowsum {
        #[frame(boundary = "\n")]
        pub ROW -> u64 = until(";" | frame_end) ";" v:u64 frame_end -> { v }
        pub FRAMED -> u64 = s:par_fold(ROW, || 0u64, |a: u64, v: u64| a + v, |a: u64, b: u64| a + b) -> { s }

        #[stream]
        pub STREAMED -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }
    }
}

const READING: Reading = Reading {
    block: 64 << 10,
    max_frame: 64 << 10,
};

fn peak_of(f: impl FnOnce() -> u64) -> (u64, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let v = f();
    (v, PEAK.load(Ordering::Relaxed) - before)
}

fn framed(bytes: usize) -> (u64, usize) {
    peak_of(|| {
        Rowsum::parse_FRAMED_reader(
            Rows::new(bytes),
            &ParseContext::<()>::default(),
            Parallelism::Off,
            READING,
        )
        .unwrap()
    })
}

fn streamed(bytes: usize) -> (u64, usize) {
    peak_of(|| {
        let mut ctx = ParseContext::<()>::default();
        Rowsum::parse_STREAMED_reader(Rows::new(bytes), &mut ctx, READING).unwrap()
    })
}

// One test, so that the two parses do not run at once and share a peak.
#[test]
fn memory_does_not_grow_with_the_input() {
    let big = 16 << 20;
    let bound = 8 * (READING.block + READING.max_frame);

    let (sum, peak) = framed(big);
    let (sum_s, peak_s) = streamed(big);
    assert_eq!(sum, sum_s);
    assert!(
        peak < bound,
        "par_fold reader: peak {peak} bytes for 16 MiB"
    );
    assert!(
        peak_s < bound,
        "#[stream] reader: peak {peak_s} bytes for 16 MiB"
    );

    let (_, small) = framed(big / 4);
    let (_, small_s) = streamed(big / 4);
    assert!(
        peak <= small + small / 2,
        "par_fold reader: {small} for 4 MiB, {peak} for 16"
    );
    assert!(
        peak_s <= small_s + small_s / 2,
        "#[stream] reader: {small_s} for 4 MiB, {peak_s} for 16"
    );
    eprintln!("peak heap: par_fold reader {peak} B, #[stream] reader {peak_s} B (bound {bound} B)");
}
