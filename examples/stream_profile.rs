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
//! | `slice` | the same rule over the input all in memory (`parse_ROWS_slice`): the plain steps |
//! | `frames_memory` | `par_fold` in memory, `Parallelism::Off` |
//! | `frames_reader` | its reader, `Parallelism::Off` |
//! | `list` | 20 000 `#[stream]` readers of `[1,2,…,10]`: a prefix step, ten items and a suffix step each, 200 000 items in all, blocks of 64 bytes |

use std::io::Cursor;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::{Parallelism, Reading};
use winnow_grammar::{ParseContext, ParseInput};

pub const ROWS: usize = 200_000;

// The benchmark's grammars.
#[path = "../benches/shared/stream.rs"]
mod shared;
use shared::{Plain, Tracked};

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
        "slice" => {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_ROWS_slice(rows.as_bytes(), &mut ctx).unwrap()
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
        "list" => {
            let list = "[1,2,3,4,5,6,7,8,9,10]";
            let mut sum = 0u64;
            for _ in 0..ROWS / 10 {
                let mut ctx = ParseContext::<()>::default();
                let s = Tracked::parse_list_reader(
                    Cursor::new(list.as_bytes()),
                    &mut ctx,
                    Reading {
                        block: 64,
                        ..Reading::default()
                    },
                )
                .unwrap();
                sum += s as u64;
            }
            sum
        }
        _ => {
            eprintln!(
                "usage: stream_profile none|plain|memory|stream|frames_memory|frames_reader|list"
            );
            std::process::exit(2);
        }
    };
    println!("{sum}");
}
