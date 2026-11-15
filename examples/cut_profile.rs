//! The callgrind half of `benches/cut.rs`: one parse of 200 000 statements
//! `let vN = N;`, chosen by the first argument, so that instructions per
//! statement can be counted. `scripts/profile-cut.sh` runs every case and
//! subtracts `none`, the cost of building the input. The numbers are in
//! ADR 15.
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `stmts_cut` / `stmts_none` | an item rule with and without a cut after `let`, in memory |
//! | `grouped_cut` / `grouped_none` | the same sequence inside a repetition (combinators) |
//! | `stream_cut` / `stream_none` | a `#[stream]` reader, `[` … `]` around the items, with and without a cut after `[` (`Diagnose::Replay`) |

use std::io::Cursor;
use winnow_grammar::rt::Reading;
use winnow_grammar::ParseContext;

#[path = "../benches/shared/cut.rs"]
mod shared;
use shared::{memory, stmts, Cuts, NoCuts};

pub const N: usize = 200_000;

fn main() {
    let text = stmts(N);
    let doc = format!("[{text}]");
    let case = std::env::args().nth(1).unwrap_or_default();
    let reader = |cut: bool| {
        let mut ctx = ParseContext::<()>::default();
        let r = Cursor::new(doc.as_bytes());
        if cut {
            Cuts::parse_doc_reader(r, &mut ctx, Reading::default()).unwrap()
        } else {
            NoCuts::parse_doc_reader(r, &mut ctx, Reading::default()).unwrap()
        }
    };
    let n = match case.as_str() {
        "none" => N,
        "stmts_cut" => memory(Cuts::parse_stmts(), &text),
        "stmts_none" => memory(NoCuts::parse_stmts(), &text),
        "grouped_cut" => memory(Cuts::parse_grouped(), &text),
        "grouped_none" => memory(NoCuts::parse_grouped(), &text),
        "stream_cut" => reader(true),
        "stream_none" => reader(false),
        _ => {
            eprintln!("usage: cut_profile none|stmts_cut|stmts_none|grouped_cut|grouped_none|stream_cut|stream_none");
            std::process::exit(2);
        }
    };
    assert_eq!(n, N);
    println!("{n}");
}
