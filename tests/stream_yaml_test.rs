//! YAML read from a stream (ADR 27 §3) - the case the feature was asked for.
//!
//! Two shapes, over the YAML subset of `tests/yaml_indent_test.rs`, whose
//! block structure is indentation passed down as values (ADR 26):
//!
//! * **a stream of documents**: `---` begins each, directives (`%…`) may come
//!   before it and `...` may end it. The stream restarts between documents;
//!   the grammar, not a byte search, decides where one ends - which is why the
//!   frame check of ADR 24 rejects `---` as a boundary and this does not need
//!   one.
//! * **one document whose top is a sequence of many entries**: the fold sits
//!   three rules down, `SEQDOC -> TOPSEQ -> SEQ_COUNT`, its indentation
//!   detected in a prefix and handed down as an argument.
//!
//! Both are compared with the parse in memory, read a byte at a time and in
//! larger pieces, on inputs that parse and on every prefix of one.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, ParseContext, ParseError, ParseInput};

#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Scalar(String),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ctx {
    BlockIn,
    BlockOut,
}

grammar! {
    grammar Yml {
        use super::{Ctx, Yaml};

        WS -> () = "" -> { () }

        // --- A stream of documents.
        #[stream]
        pub STREAM -> Vec<Yaml> =
            docs:fold(DOCUMENT, Vec::new, |mut v: Vec<Yaml>, d: Yaml| { v.push(d); v }) eof
            -> { docs }

        // The same stream, going on past a broken document to the next
        // marker - SYNTAX.md, *A broken record*.
        #[stream]
        pub RSTREAM -> (usize, usize) =
            r:fold(RDOC, || (0usize, 0usize), |(ok, bad): (usize, usize), d: Option<Yaml>| {
                if d.is_some() { (ok + 1, bad) } else { (ok, bad + 1) }
            })
            eof -> { r }
        RDOC -> Option<Yaml> = r:recover(DOCUMENT, peek(("---" (" " | "\n")))) -> { r }

        DOCUMENT -> Yaml = DIRECTIVE* "---" "\n" v:BLOCK(n = -1, c = Ctx::BlockIn) END? -> { v }
        DIRECTIVE -> () = "%" until("\n") "\n" -> { () }
        END -> () = "..." "\n" -> { () }

        // --- One document, its top a sequence: the fold three rules down.
        #[stream]
        pub SEQDOC -> (usize, Yaml) = "---" "\n" r:TOPSEQ -> { r }
        TOPSEQ -> (usize, Yaml) = m:count(" ") r:SEQ_COUNT(m = m as isize) -> { r }
        SEQ_COUNT(m: isize) -> (usize, Yaml) =
            first:SEQ_ENTRY(m = m)
            rest:fold(SEQ_MORE(m = m), || 0usize, |n: usize, _e: Yaml| n + 1)
            -> { (rest + 1, first) }

        // --- The block structure, as in `yaml_indent_test.rs`.
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
        SEQ_ENTRY(m: isize) -> Yaml = not(FORBIDDEN) "-" v:AFTER(m = m, c = Ctx::BlockIn) -> { v }

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

        // c-forbidden: `---` or `...` at the start of a line, followed by
        // white space or the end, is a marker and never content.
        FORBIDDEN -> () = ("---" | "...") (" " | "\n" | eof) -> { () }
        KEY -> &'a str =
            not(FORBIDDEN) s:until(":" | "\n") if(!s.is_empty() && !s.starts_with(' ')) -> { s }
        SCALAR -> &'a str = not(FORBIDDEN) s:until("\n") if(!s.is_empty()) -> { s }
    }
}

struct Trickle<'d> {
    data: &'d [u8],
    step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fn agree<T: PartialEq + std::fmt::Debug>(
    what: &str,
    mem: &Result<T, ParseError>,
    got: &Result<T, ParseError>,
) {
    match (mem, got) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}"),
        (Err(a), Err(b)) => {
            assert_eq!(
                a.offset, b.offset,
                "{what}: offsets\n in memory: {a:?}\n read: {b:?}"
            );
            assert_eq!(a.headline(), b.headline(), "{what}: messages");
        }
        _ => panic!("{what}:\n in memory: {mem:?}\n read: {got:?}"),
    }
}

fn in_memory<'a, T>(
    p: &mut impl Parser<ParseInput<'a, ()>, T, ParseError>,
    input: &'a str,
) -> Result<T, ParseError> {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    })
}

const STEPS: [usize; 4] = [1, 2, 7, 1 << 20];

fn documents(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        match i % 4 {
            0 => s.push_str(&format!("---\nname: doc{i}\ntags:\n  - a\n  - b\n")),
            1 => s.push_str(&format!("%YAML 1.2\n---\n- x{i}\n- - y\n  - z\n...\n")),
            2 => s.push_str(&format!("---\nscalar {i}\n")),
            _ => s.push_str(&format!("---\nowner:\n  login: k{i}\n  repos:\n    - r\n")),
        }
    }
    s
}

fn stream(input: &str, step: usize) -> Result<Vec<Yaml>, ParseError> {
    let mut ctx = ParseContext::<()>::default();
    Yml::parse_STREAM_reader(
        Trickle {
            data: input.as_bytes(),
            step,
        },
        &mut ctx,
        Reading::default(),
    )
}

#[test]
fn a_stream_of_documents_agrees_with_the_parse_in_memory() {
    let input = documents(40);
    let mem = in_memory(&mut Yml::parse_STREAM(), &input);
    assert_eq!(mem.as_ref().unwrap().len(), 40);
    for step in STEPS {
        agree(&format!("step {step}"), &mem, &stream(&input, step));
    }
}

#[test]
fn a_marker_inside_a_document_ends_it_and_nothing_else_does() {
    // `---x` at the start of a line is a scalar, `--- ` and `---\n` are
    // markers: the grammar decides, so a byte search would have cut wrong.
    let input = "---\n- ---x\n- b\n---\nk: v\n";
    let mem = in_memory(&mut Yml::parse_STREAM(), input);
    assert_eq!(mem.as_ref().unwrap().len(), 2, "{mem:?}");
    for step in STEPS {
        agree(&format!("step {step}"), &mem, &stream(input, step));
    }
}

#[test]
fn every_prefix_of_a_stream_agrees_with_the_parse_in_memory() {
    let input = documents(5);
    for i in 0..=input.len() {
        let prefix = &input[..i];
        let mem = in_memory(&mut Yml::parse_STREAM(), prefix);
        for step in [1, 3] {
            agree(
                &format!("{prefix:?}, step {step}"),
                &mem,
                &stream(prefix, step),
            );
        }
    }
}

fn sequence(n: usize) -> String {
    let mut s = String::from("---\n");
    for i in 0..n {
        match i % 3 {
            0 => s.push_str(&format!("- item {i}\n")),
            1 => s.push_str(&format!("- name: n{i}\n  size: {i}\n")),
            _ => s.push_str(&format!("- - inner {i}\n  - more\n")),
        }
    }
    s
}

fn seqdoc(input: &str, step: usize, reading: Reading) -> Result<(usize, Yaml), ParseError> {
    let mut ctx = ParseContext::<()>::default();
    Yml::parse_SEQDOC_reader(
        Trickle {
            data: input.as_bytes(),
            step,
        },
        &mut ctx,
        reading,
    )
}

#[test]
fn one_document_with_a_long_sequence_is_read_an_entry_at_a_time() {
    let input = sequence(500);
    let mem = in_memory(&mut Yml::parse_SEQDOC(), &input);
    assert_eq!(mem.as_ref().unwrap().0, 500);
    for step in STEPS {
        agree(
            &format!("step {step}"),
            &mem,
            &seqdoc(&input, step, Reading::default()),
        );
    }
    // An entry is at most a few dozen bytes, so a window of 64 is enough.
    let small = Reading {
        block: 16,
        max_frame: 64,
    };
    agree("max_frame 64", &mem, &seqdoc(&input, 5, small));
}

#[test]
fn every_prefix_of_a_sequence_document_agrees_with_the_parse_in_memory() {
    let input = sequence(6);
    let mut variants = vec![input.clone()];
    for i in 0..input.len() {
        variants.push(input[..i].to_string());
        let mut b = input.clone().into_bytes();
        if b[i] == b' ' {
            b[i] = b'x';
            variants.push(String::from_utf8(b).unwrap());
        }
    }
    for v in variants {
        let mem = in_memory(&mut Yml::parse_SEQDOC(), &v);
        for step in [1, 4] {
            agree(
                &format!("{v:?}, step {step}"),
                &mem,
                &seqdoc(&v, step, Reading::default()),
            );
        }
    }
}

#[test]
fn a_broken_document_is_skipped_to_the_next_marker() {
    // The second document is the scalar `-x` - plain, as `-` is not
    // followed by a space - and then `- y`, which no document begins with:
    // that one item fails, is skipped to the next marker, and the third
    // document is read as if nothing had happened.
    let input = "---\na: 1\n---\n-x\n- y\n---\n- z\n";
    let mem = in_memory(&mut Yml::parse_RSTREAM(), input);
    assert_eq!(mem.as_ref().unwrap(), &(3, 1), "{mem:?}");
    for step in STEPS {
        let mut ctx = ParseContext::<()>::default();
        let got = Yml::parse_RSTREAM_reader(
            Trickle {
                data: input.as_bytes(),
                step,
            },
            &mut ctx,
            Reading::default(),
        );
        agree(&format!("step {step}"), &mem, &got);
        assert_eq!(ctx.recoveries, 1);
    }
}
