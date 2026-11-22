//! The `#[stream]` entries over a `&str` (`parse_<rule>_str`): the slice
//! entry without the second check of the input as UTF-8. The same parses as
//! `slice_profile`'s `slice` and `list`, chosen by the first argument, so
//! that the two can be set side by side.
//!
//! An example of its own: a case that uses an entry point base does not
//! have keeps the whole example from building there (see `slice_profile`).
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `slice` | 200 000 rows of `station12;345\n` (`parse_ROWS_str`) |
//! | `list` | 20 000 inputs `[1,2,…,10]`, a context each (`parse_list_str`) |

use winnow_grammar::ParseContext;

pub const ROWS: usize = 200_000;

// The benchmark's grammars.
#[path = "../benches/shared/stream.rs"]
mod shared;
use shared::Tracked;

fn main() {
    let rows = "station12;345\n".repeat(ROWS);
    let case = std::env::args().nth(1).unwrap_or_default();
    let sum = match case.as_str() {
        "none" => 0,
        "slice" => {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_ROWS_str(&rows, &mut ctx).unwrap()
        }
        "list" => {
            let list = "[1,2,3,4,5,6,7,8,9,10]";
            let mut sum = 0u64;
            for _ in 0..ROWS / 10 {
                let mut ctx = ParseContext::<()>::default();
                sum += Tracked::parse_list_str(list, &mut ctx).unwrap() as u64;
            }
            sum
        }
        _ => {
            eprintln!("usage: str_profile none|slice|list");
            std::process::exit(2);
        }
    };
    println!("{sum}");
}
