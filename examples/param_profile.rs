//! Rules with value parameters, for callgrind: one parse of a YAML document
//! of 20 000 block mappings, chosen by the first argument. Every rule but the
//! entry point takes the indentation `m: isize` and some the context
//! `c: Ctx`, so the arguments are passed on at nearly every call - the code
//! the other profiles have none of. `scripts/compare-perf.sh` runs every
//! case.
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `yaml` | the parse of the whole document |

use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseInput};

pub const N: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Scalar(String),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

/// The spec's block contexts that matter here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ctx {
    /// A sequence entry's content.
    BlockIn,
    /// A mapping value.
    BlockOut,
}

// The grammar of `tests/yaml_indent_test.rs`, where it is explained.
grammar! {
    grammar Yml {
        use super::{Ctx, Yaml};

        WS -> () = "" -> { () }

        pub DOC -> Yaml = v:BLOCK(n = -1, c = Ctx::BlockIn) eof -> { v }

        BLOCK(n: isize, c: Ctx) -> Yaml =
            m:count(" ") if((m as isize) > n) v:CONTENT(m = m as isize) -> { v }
          | m:count(" ") if(c == Ctx::BlockOut && (m as isize) == n) s:SEQ(m = n) -> { Yaml::Seq(s) }

        CONTENT(m: isize) -> Yaml =
            s:SEQ(m = m) -> { Yaml::Seq(s) }
          | e:MAP(m = m) -> { Yaml::Map(e) }
          | s:SCALAR "\n" -> { Yaml::Scalar(s.to_string()) }

        SEQ(m: isize) -> Vec<Yaml> =
            first:SEQ_ENTRY(m = m) rest:SEQ_MORE(m = m)* -> {
                let mut all = vec![first];
                all.extend(rest);
                all
            }
        SEQ_MORE(m: isize) -> Yaml = " "{m} e:SEQ_ENTRY(m = m) -> { e }
        SEQ_ENTRY(m: isize) -> Yaml = "-" v:AFTER(m = m, c = Ctx::BlockIn) -> { v }

        MAP(m: isize) -> Vec<(String, Yaml)> =
            first:MAP_ENTRY(m = m) rest:MAP_MORE(m = m)* -> {
                let mut all = vec![first];
                all.extend(rest);
                all
            }
        MAP_MORE(m: isize) -> (String, Yaml) = " "{m} e:MAP_ENTRY(m = m) -> { e }
        MAP_ENTRY(m: isize) -> (String, Yaml) =
            k:KEY ":" v:AFTER(m = m, c = Ctx::BlockOut) -> { (k.to_string(), v) }

        AFTER(m: isize, c: Ctx) -> Yaml =
            "\n" v:BLOCK(n = m, c = c) -> { v }
          | s:count(" ") if(s > 0 && c == Ctx::BlockIn, "a sequence entry")
            v:CONTENT(m = m + 1 + s as isize) -> { v }
          | s:count(" ") if(s > 0) v:SCALAR "\n" -> { Yaml::Scalar(v.to_string()) }

        KEY -> &'a str = s:until(":" | "\n") if(!s.is_empty() && !s.starts_with(' ')) -> { s }
        SCALAR -> &'a str = s:until("\n") if(!s.is_empty()) -> { s }
    }
}

/// `N` mappings with nested mappings, sequences and a compact sequence.
fn document() -> String {
    let mut doc = String::new();
    for i in 0..N {
        doc.push_str(&format!(
            "k{i}:\n  name: x\n  tags:\n    - a\n    - - b\n      - c\n  owner:\n    login: y\n"
        ));
    }
    doc
}

fn main() {
    let doc = document();
    let case = std::env::args().nth(1).unwrap_or_default();
    let n = match case.as_str() {
        "none" => N,
        "yaml" => {
            let parsed = Yml::parse_DOC()
                .parse_next(&mut ParseInput {
                    input: LocatingSlice::new(doc.as_str()),
                    state: ParseContext::<()>::default(),
                })
                .unwrap();
            match parsed {
                Yaml::Map(entries) => entries.len(),
                _ => 0,
            }
        }
        _ => {
            eprintln!("usage: param_profile none|yaml");
            std::process::exit(2);
        }
    };
    assert_eq!(n, N);
    println!("{n}");
}
