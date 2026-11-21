//! A `#[stream]` rule run over an input all in memory
//! (`parse_<rule>_slice`), next to the same rule's parse in memory and its
//! reader: one parse of 200 000 rows of `station12;345\n`, chosen by the
//! first argument, so that instructions per row can be counted.
//!
//! An example of its own rather than cases of `stream_profile`: a case that
//! uses an entry point base does not have keeps the whole example from
//! building there, and `scripts/compare-perf.sh` then compares none of its
//! cases.
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `memory` | the fold in memory, in a grammar with a `#[stream]` rule |
//! | `slice` | the same rule over the input all in memory (`parse_ROWS_slice`): the plain steps |
//! | `list` | 20 000 slices `[1,2,…,10]`: a prefix step, ten items and a suffix step each |
//! | `doc_none` / `doc_cut` | `[` 200 000 statements `]`, without and with a cut after `[` that stays open across the fold (cut_profile's grammars) |

use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{ParseContext, ParseInput};

pub const ROWS: usize = 200_000;

// The benchmark's grammars.
#[path = "../benches/shared/stream.rs"]
mod shared;
use shared::Tracked;

#[path = "../benches/shared/cut.rs"]
#[allow(dead_code)]
mod cut;

fn main() {
    let rows = "station12;345\n".repeat(ROWS);
    let case = std::env::args().nth(1).unwrap_or_default();
    let sum = match case.as_str() {
        "none" => 0,
        "memory" => {
            let mut i = ParseInput {
                input: LocatingSlice::new(rows.as_str()),
                state: ParseContext::<()>::default(),
            };
            Tracked::parse_ROWS().parse_next(&mut i).unwrap()
        }
        "slice" => {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_ROWS_slice(rows.as_bytes(), &mut ctx).unwrap()
        }
        "list" => {
            let list = "[1,2,3,4,5,6,7,8,9,10]";
            let mut sum = 0u64;
            for _ in 0..ROWS / 10 {
                let mut ctx = ParseContext::<()>::default();
                sum += Tracked::parse_list_slice(list.as_bytes(), &mut ctx).unwrap() as u64;
            }
            sum
        }
        "doc_none" | "doc_cut" => {
            let doc = format!("[{}]", cut::stmts(ROWS));
            let mut ctx = ParseContext::<()>::default();
            let n = if case == "doc_cut" {
                cut::Cuts::parse_doc_slice(doc.as_bytes(), &mut ctx)
            } else {
                cut::NoCuts::parse_doc_slice(doc.as_bytes(), &mut ctx)
            };
            n.unwrap() as u64
        }
        _ => {
            eprintln!("usage: slice_profile none|memory|slice|list|doc_none|doc_cut");
            std::process::exit(2);
        }
    };
    println!("{sum}");
}
