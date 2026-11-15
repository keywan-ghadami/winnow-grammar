//! The grammars `benches/stream.rs` measures and
//! `examples/stream_profile.rs` counts instructions of - one file, so that
//! the profile and the benchmark cannot drift apart. Each includes it with
//! `#[path]`.

use winnow_grammar::grammar;

grammar! {
    grammar Plain {
        #[frame(boundary = "\n")]
        pub ROW -> u64 = until(";" | frame_end) ";" v:u64 frame_end -> { v }
        pub FILE -> u64 = s:par_fold(ROW, || 0u64, |a: u64, v: u64| a + v, |a: u64, b: u64| a + b) -> { s }

        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }

        pub list -> i64 = "[" s:fold(num, || 0i64, |a: i64, n: i64| a + n) "]" -> { s }
        num -> i64 = n:i64 ","? -> { n }
    }
}

grammar! {
    grammar Tracked {
        #[stream]
        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }

        #[stream]
        pub list -> i64 = "[" s:fold(num, || 0i64, |a: i64, n: i64| a + n) "]" -> { s }
        num -> i64 = n:i64 ","? -> { n }
    }
}
