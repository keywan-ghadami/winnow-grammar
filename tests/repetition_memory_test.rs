//! What a repetition allocates, by what the grammar does with its result.
//!
//! A repetition whose elements are bound has to produce them, and the `Vec`
//! is the point. One whose result is discarded (`x*` with no binding) or only
//! counted (`count(x)`) has an answer of constant size, and the grammar has
//! already said so - it named no elements. Collecting there is memory spent
//! for nothing, and on a large input it is the memory that matters, not the
//! nanoseconds: `fold` exists in this crate for exactly that reason.
//!
//! Timing says nothing here - measured, the collecting and non-collecting
//! forms are within noise of each other (`benches/repetition.rs`). Peak
//! allocation is the property worth pinning, so this test counts bytes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseInput};

/// Tracks bytes currently held and the high-water mark.
struct Tracking;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK.fetch_max(live, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Tracking = Tracking;

/// Runs `f` and returns the high-water mark of bytes held during it.
///
/// The counters are global and `cargo test` runs tests on threads, so the
/// measurement has to be the only one running - otherwise a peak includes
/// whatever a sibling test allocated at the same moment.
fn peak_bytes(f: impl FnOnce()) -> usize {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let before = LIVE.load(Relaxed);
    PEAK.store(before, Relaxed);
    f();
    PEAK.load(Relaxed).saturating_sub(before)
}

/// Parses `input` with `p`, allocating nothing the parse does not.
fn parse<'a, O>(
    mut p: impl Parser<ParseInput<'a, ()>, O, winnow_grammar::ParseError>,
    input: &'a str,
) -> O {
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    p.parse_next(&mut stream).expect("the input is all digits")
}

grammar! {
    grammar Rep {
        // Lexical: one element per digit, no separators.
        pub BOUND -> usize = xs:digit* -> { xs.len() }
        pub DISCARDED -> () = digit* -> { () }
        pub COUNTED -> usize = n:count(digit) -> { n }
        pub BOUNDED_DISCARDED -> () = digit{1000,} -> { () }
    }
}

const N: usize = 200_000;

/// Built once for the whole binary: allocating it inside a test would land
/// in a sibling's measured window, and it is larger than the threshold.
fn digits() -> &'static str {
    static INPUT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INPUT.get_or_init(|| (0..N).map(|i| char::from(b'0' + (i % 10) as u8)).collect())
}

/// The baseline: bound elements are produced, so the memory is the answer.
/// `Vec<char>` over `N` elements is at least `4 * N` bytes.
#[test]
fn a_bound_repetition_holds_its_elements() {
    let input = digits();
    let peak = peak_bytes(|| {
        assert_eq!(parse(Rep::parse_BOUND(), input), N);
    });
    assert!(
        peak >= 4 * N,
        "a bound repetition should hold its {N} elements; peak was {peak} bytes"
    );
}

/// Nothing names the result, so nothing has to be held. The bound is generous
/// - what matters is that it does not grow with the input.
#[test]
fn a_discarded_repetition_holds_nothing() {
    let input = digits();
    let peak = peak_bytes(|| {
        parse(Rep::parse_DISCARDED(), input);
    });
    assert!(
        peak < N,
        "a discarded repetition should not grow with its {N} elements; peak was {peak} bytes"
    );
}

/// `count(p)` answers with one number.
#[test]
fn a_counted_repetition_holds_nothing() {
    let input = digits();
    let peak = peak_bytes(|| {
        assert_eq!(parse(Rep::parse_COUNTED(), input), N);
    });
    assert!(
        peak < N,
        "`count(p)` should not grow with its {N} elements; peak was {peak} bytes"
    );
}

/// A bounded repetition is no different when its result is discarded.
#[test]
fn a_discarded_bounded_repetition_holds_nothing() {
    let input = digits();
    let peak = peak_bytes(|| {
        parse(Rep::parse_BOUNDED_DISCARDED(), input);
    });
    assert!(
        peak < N,
        "a discarded `x{{1000,}}` should not grow with its {N} elements; peak was {peak} bytes"
    );
}
