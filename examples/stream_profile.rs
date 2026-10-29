//! The callgrind half of ADR 27 §5: one parse of 200 000 rows of
//! `station12;345\n`, chosen by the first argument, so that instructions per
//! row can be counted. `scripts/profile-stream.sh` runs every case and
//! subtracts `none`, the cost of building the input.
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `plain` | the fold in memory, no edge tracking |
//! | `memory` | the same fold in memory, in a grammar with a `#[stream]` rule |
//! | `stream` | its `#[stream]` reader over a `Cursor` |
//! | `frames_memory` | `par_fold` in memory, `Parallelism::Off` |
//! | `frames_reader` | its reader, `Parallelism::Off` |

use std::io::Cursor;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::{Parallelism, Reading};
use winnow_grammar::{grammar, ParseContext, ParseInput};

pub const ROWS: usize = 200_000;

grammar! {
    grammar Plain {
        #[frame(boundary = "\n")]
        pub ROW -> u64 = until(";" | frame_end) ";" v:u64 frame_end -> { v }
        pub FILE -> u64 = s:par_fold(ROW, || 0u64, |a: u64, v: u64| a + v, |a: u64, b: u64| a + b) -> { s }
        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }
    }
}

grammar! {
    grammar Tracked {
        #[stream]
        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }
    }
}

fn main() {
    let rows = "station12;345\n".repeat(ROWS);
    let case = std::env::args().nth(1).unwrap_or_default();
    let in_memory = |rows: &str, tracked: bool| {
        let mut i = ParseInput {
            input: LocatingSlice::new(rows),
            state: ParseContext::<()>::default(),
        };
        if tracked {
            Tracked::parse_ROWS().parse_next(&mut i).unwrap()
        } else {
            Plain::parse_ROWS().parse_next(&mut i).unwrap()
        }
    };
    let sum = match case.as_str() {
        "none" => 0,
        "plain" => in_memory(&rows, false),
        "memory" => in_memory(&rows, true),
        "stream" => {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_ROWS_reader(Cursor::new(rows.as_bytes()), &mut ctx, Reading::default())
                .unwrap()
        }
        "frames_memory" => {
            Plain::parse_FILE_pieces(&rows, &ParseContext::<()>::default(), Parallelism::Off)
                .unwrap()
        }
        "frames_reader" => Plain::parse_FILE_reader(
            Cursor::new(rows.as_bytes()),
            &ParseContext::<()>::default(),
            Parallelism::Off,
            Reading::default(),
        )
        .unwrap(),
        _ => {
            eprintln!("usage: stream_profile none|plain|memory|stream|frames_memory|frames_reader");
            std::process::exit(2);
        }
    };
    println!("{sum}");
}
