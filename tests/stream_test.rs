//! `#[stream]` and `parse_<rule>_reader` (ADR 27 §3): a grammar's input read
//! from a `Read` one item of a fold at a time.
//!
//! The property every test here checks is the one the feature promises:
//! **the reader's answer is the in-memory parse's** - the value, or the
//! error at the same offset with the same message - however the input
//! arrives: a byte at a time, in odd sizes, all at once. The only other
//! answer allowed is "looked further ahead than `max_frame`", and only when
//! `max_frame` is smaller than what the input needs.
//!
//! Reading a byte at a time is what makes this a test of the edge tracking:
//! every step is first tried over whatever has arrived, which ends anywhere
//! inside an item. A parser that depended on that end without noting it
//! would hand back a result the whole input would not have given, and the
//! comparison would show it.

use std::fmt::Debug;
use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseBytes, ParseContext, ParseError, ParseInput};

/// Hands out at most `step` bytes per read, and every third call nothing but
/// `Interrupted`.
struct Trickle<'d> {
    data: &'d [u8],
    step: usize,
    calls: usize,
}

impl<'d> Trickle<'d> {
    fn new(data: &'d [u8], step: usize) -> Self {
        Trickle {
            data,
            step,
            calls: 0,
        }
    }
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.calls += 1;
        if self.calls.is_multiple_of(3) {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = self.step.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fn reading(block: usize, max_frame: usize) -> Reading {
    Reading { block, max_frame }
}

const BIG: usize = 1 << 20;

fn too_far(e: &ParseError) -> bool {
    e.headline().contains("looked more than")
}

/// The reader agrees with the parse in memory - or, where `max_frame` may be
/// too small, reports exactly that.
fn agree<T: PartialEq + Debug>(
    what: &str,
    mem: &Result<T, ParseError>,
    got: &Result<T, ParseError>,
    may_be_too_far: bool,
) {
    match (mem, got) {
        (_, Err(g)) if may_be_too_far && too_far(g) => {}
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

fn mem_text<T>(
    p: &mut impl Parser<ParseInput<'static, ()>, T, ParseError>,
    input: &'static str,
    diagnose: Diagnose,
) -> Result<T, ParseError> {
    let ctx = ParseContext::<()> {
        diagnose,
        ..Default::default()
    };
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ctx,
    })
}

/// A tiny deterministic generator, for inputs made of random pieces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as usize
    }
}

fn pieces(seed: u64, count: usize, alphabet: &[&str], max_len: usize) -> Vec<String> {
    let mut rng = Rng(seed);
    (0..count)
        .map(|_| {
            let n = rng.next() % (max_len + 1);
            (0..n)
                .map(|_| alphabet[rng.next() % alphabet.len()])
                .collect()
        })
        .collect()
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

// --- Many kinds of parser as the items of one fold -------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Kw(&'static str),
    Let(String),
    Float(u64),
    Int(i64),
    Str(String),
    Char(char),
    Bool(bool),
    Comment(String),
    Digits(String),
    Xs(usize),
    Paren(char),
    At,
    Span(std::ops::Range<usize>),
    Word(winnow_grammar::Symbol),
}

grammar! {
    grammar Leaves {
        use super::Item;

        #[stream]
        pub file -> Vec<Item> =
            items:fold(item, Vec::new, |mut v: Vec<Item>, i: Item| { v.push(i); v }) -> { items }

        item -> Item =
            "abc" -> { Item::Kw("abc") }
          | "ab" "!" -> { Item::Kw("ab!") }
          | "let" n:raw_ident -> { Item::Let(n.to_string()) }
          | f:f64 "f" -> { Item::Float(f.to_bits()) }
          | n:i64 -> { Item::Int(n) }
          | s:string -> { Item::Str(s.to_string()) }
          | c:char -> { Item::Char(c) }
          | b:bool -> { Item::Bool(b) }
          | t:COMMENT -> { Item::Comment(t.to_string()) }
          | "<" d:text(digit+) ">" -> { Item::Digits(d.to_string()) }
          | "{" n:count("x") "}" -> { Item::Xs(n) }
          | "(" not("))") c:any ")" -> { Item::Paren(c) }
          | "@" peek("@@") "@@" -> { Item::At }
          | "%" s:SPANNED -> { s }
          | "$" w:ident -> { Item::Word(w) }

        COMMENT -> &'a str = "#" t:until(line_ending) line_ending -> { t }
        SPANNED -> Item = (digit+) @ s -> { Item::Span(s) }
    }
}

const LEAF_PIECES: &[&str] = &[
    "abc",
    "ab!",
    "ab",
    "a",
    "let x",
    "let",
    "letter",
    "1.5f",
    "12",
    "-3",
    "1e3f",
    "1.",
    "\"s\\\"q\"",
    "\"open",
    "\"",
    "'c'",
    "'\\n'",
    "'",
    "true",
    "false",
    "fals",
    "# c\n",
    "#",
    "<12>",
    "<1",
    "{xxx}",
    "{x",
    "(a)",
    "(",
    "))",
    "@@@",
    "@",
    "%123",
    "%",
    "$name",
    "$",
    " ",
    "\n",
    "  ",
    "x",
    "!",
    ")",
    "é",
];

fn leaves_read(
    input: &str,
    step: usize,
    block: usize,
    max_frame: usize,
) -> Result<Vec<Item>, ParseError> {
    let mut ctx = ParseContext::<()>::default();
    Leaves::parse_file_reader(
        Trickle::new(input.as_bytes(), step),
        &mut ctx,
        reading(block, max_frame),
    )
}

fn leaves_check(input: &'static str) {
    let mem = mem_text(&mut Leaves::parse_file(), input, Diagnose::default());
    for (step, block, max_frame) in [
        (1, 1, BIG),
        (2, 3, BIG),
        (7, 1, BIG),
        (BIG, BIG, BIG),
        (1, 1, 0),
        (1, 2, 3),
        (3, 1, 6),
    ] {
        let got = leaves_read(input, step, block, max_frame);
        agree(
            &format!("{input:?}, step {step}, block {block}, max_frame {max_frame}"),
            &mem.as_ref()
                .map(|v| without_symbols(v))
                .map_err(Clone::clone),
            &got.as_ref()
                .map(|v| without_symbols(v))
                .map_err(Clone::clone),
            max_frame != BIG,
        );
    }
}

/// Symbols are numbers of an interner; each parse here has its own.
fn without_symbols(v: &[Item]) -> Vec<Item> {
    v.iter()
        .map(|i| match i {
            Item::Word(_) => Item::Kw("$word"),
            other => other.clone(),
        })
        .collect()
}

#[test]
fn every_kind_of_parser_agrees_with_the_parse_in_memory() {
    let corpus = [
        "",
        " ",
        "abc ab! let x 1.5f 12 -3 \"s\\\"q\" 'c' true # c\n<12> {xxx} (a) @@@ %42 $name",
        "abc",
        "ab",
        "ab!",
        "1.5",
        "1.5f",
        "1e",
        "\"open",
        "\"a\\\"",
        "'",
        "fals",
        "falsex",
        "# no end",
        "<1",
        "{xx",
        "(",
        "@@",
        "%",
        "$",
        "let",
        "letter",
        "abc abc abc abc abc abc abc abc abc abc abc abc abc abc abc abc abc abc x",
        "éé",
        "12 é",
    ];
    for input in corpus {
        leaves_check(input);
    }
}

#[test]
fn random_inputs_agree_with_the_parse_in_memory() {
    for input in pieces(0x5eed, 1500, LEAF_PIECES, 8) {
        leaves_check(leak(input));
    }
}

#[test]
fn every_short_input_over_a_small_alphabet_agrees() {
    let alphabet = ["a", "b", "c", "!", "1", ".", "f", " ", "\""];
    let mut inputs = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..4 {
        let mut next = Vec::new();
        for s in &frontier {
            for c in alphabet {
                next.push(format!("{s}{c}"));
            }
        }
        inputs.extend(next.iter().cloned());
        frontier = next;
    }
    for input in inputs {
        let input = leak(input);
        let mem = mem_text(&mut Leaves::parse_file(), input, Diagnose::default());
        let got = leaves_read(input, 1, 1, BIG);
        agree(&format!("{input:?}"), &mem, &got, false);
    }
}

#[test]
fn spans_are_positions_in_the_whole_input() {
    let input = "abc %12 abc abc %345";
    let got = leaves_read(input, 1, 1, BIG).unwrap();
    assert_eq!(got[1], Item::Span(5..7));
    assert_eq!(got[4], Item::Span(17..20));
}

#[test]
fn a_window_too_small_for_an_item_is_reported_as_such() {
    let input = "abc \"a long string that does not fit\" abc";
    let e = leaves_read(input, 1, 1, 8).unwrap_err();
    assert!(too_far(&e), "{e:?}");
    assert_eq!(
        e.offset, 3,
        "the step that needed more began after the first item"
    );
    leaves_read(input, 1, 1, 64).unwrap();
}

#[test]
fn every_diagnose_mode_reports_what_the_parse_in_memory_reports() {
    for input in ["abc ab! 12 x", "abc {xx", "\"open", "abc (", "ab"] {
        for mode in [
            Diagnose::Replay,
            Diagnose::ReplayInPlace,
            Diagnose::Eager,
            Diagnose::Off,
        ] {
            let mem = mem_text(&mut Leaves::parse_file(), input, mode);
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            let got = Leaves::parse_file_reader(
                Trickle::new(input.as_bytes(), 1),
                &mut ctx,
                reading(1, BIG),
            );
            match (&mem, &got) {
                (Err(a), Err(b)) if mode == Diagnose::Off => {
                    assert!(a.is_undiagnosed() && b.is_undiagnosed(), "{input:?}")
                }
                _ => agree(&format!("{input:?}, {mode:?}"), &mem, &got, false),
            }
        }
    }
}

// --- The path: a fold below the top, values passed down (ADR 26) -----------

#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub title: String,
    pub count: usize,
    pub sum: i64,
    pub tail: u32,
}

grammar! {
    grammar Records {
        use super::Doc;

        // `{"title": "…", "items": [n, …], "tail": 7}`: one document whose
        // items are the large part.
        #[stream]
        pub doc -> Doc =
            "{" t:title "," body:items(scale = 10) "," "\"tail\"" ":" tail:u32 "}"
            -> { Doc { title: t, count: body.0, sum: body.1, tail } }

        title -> String = "\"title\"" ":" s:string -> { s.to_string() }

        items(scale: i64) -> (usize, i64) =
            "\"items\"" ":" "[" first:entry(scale = scale)
            rest:fold(more(scale = scale), || (0usize, 0i64), |(n, s): (usize, i64), v: i64| (n + 1, s + v))
            "]"
            -> { (rest.0 + 1, rest.1 + first) }

        entry(scale: i64) -> i64 = n:i64 -> { n * scale }
        more(scale: i64) -> i64 = "," n:entry(scale = scale) -> { n }
    }
}

fn records_input(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| (i as i64 * 7 - 50).to_string()).collect();
    format!(
        "{{ \"title\": \"a \\\"quoted\\\" title\", \"items\": [ {} ], \"tail\": 7 }}",
        items.join(", ")
    )
}

#[test]
fn a_fold_below_the_top_is_streamed_with_its_values() {
    let input = leak(records_input(300));
    let mem = mem_text(&mut Records::parse_doc(), input, Diagnose::default());
    assert_eq!(mem.as_ref().unwrap().count, 300);
    for (step, block, max_frame) in [(1, 1, BIG), (5, 3, BIG), (BIG, BIG, BIG), (1, 1, 16)] {
        let mut ctx = ParseContext::<()>::default();
        let got = Records::parse_doc_reader(
            Trickle::new(input.as_bytes(), step),
            &mut ctx,
            reading(block, max_frame),
        );
        agree(
            &format!("step {step}, block {block}"),
            &mem,
            &got,
            max_frame != BIG,
        );
    }
}

#[test]
fn errors_on_the_path_agree_with_the_parse_in_memory() {
    let good = records_input(5);
    let mut inputs = vec![good.clone()];
    // Every prefix of a good input, and the good input with one byte
    // changed at every position.
    for i in 0..good.len() {
        inputs.push(good[..i].to_string());
        for c in *b"x, ]" {
            let mut b = good.clone().into_bytes();
            b[i] = c;
            if let Ok(s) = String::from_utf8(b) {
                inputs.push(s);
            }
        }
    }
    for input in inputs {
        let input = leak(input);
        let mem = mem_text(&mut Records::parse_doc(), input, Diagnose::default());
        for step in [1, 4, BIG] {
            let mut ctx = ParseContext::<()>::default();
            let got = Records::parse_doc_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("{input:?}, step {step}"), &mem, &got, false);
        }
    }
}

// --- A user state: actions that write it run once per item ---------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub words: Vec<String>,
}

grammar! {
    grammar Counting {
        use super::Seen;
        state Seen;

        #[stream]
        pub words -> usize = n:fold(word, || 0usize, |n: usize, _w: ()| n + 1) -> { n }
        word -> () = w:raw_ident -> { _state.user().words.push(w.to_string()); }
    }
}

#[test]
fn an_action_that_writes_the_state_runs_once_per_item() {
    let input = "alpha beta gamma delta epsilon";
    for mode in [Diagnose::Replay, Diagnose::ReplayInPlace] {
        for step in [1, 3, BIG] {
            let mut ctx = ParseContext::with_state(Seen::default());
            ctx.diagnose = mode;
            let n = Counting::parse_words_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            )
            .unwrap();
            assert_eq!(n, 5);
            assert_eq!(
                ctx.user_state.words,
                ["alpha", "beta", "gamma", "delta", "epsilon"],
                "{mode:?}, step {step}"
            );
        }
    }
}

// --- A byte grammar --------------------------------------------------------

grammar! {
    grammar Lengths {
        input bytes;

        // Length-prefixed records: `3:abc`.
        #[stream]
        pub records -> (usize, usize) =
            r:fold(record, || (0usize, 0usize), |(n, t): (usize, usize), b: &[u8]| (n + 1, t + b.len()))
            -> { r }
        record -> &'a [u8] = n:usize ":" b:bytes(byte{n}) -> { b }
    }
}

#[test]
fn a_byte_grammar_streams_bytes() {
    let mut input = Vec::new();
    for i in 0..200usize {
        let payload: Vec<u8> = (0..i % 17).map(|j| (i * 31 + j) as u8).collect();
        input.extend(format!("{}:", payload.len()).into_bytes());
        input.extend(payload);
    }
    let mem = Lengths::parse_records().parse_next(&mut ParseBytes {
        input: LocatingSlice::new(&input[..]),
        state: ParseContext::<()>::default(),
    });
    for step in [1, 5, BIG] {
        let mut ctx = ParseContext::<()>::default();
        let got =
            Lengths::parse_records_reader(Trickle::new(&input, step), &mut ctx, reading(1, BIG));
        agree(&format!("step {step}"), &mem, &got, false);
    }
    // Cut short in the middle of a payload.
    let short = &input[..input.len() - 3];
    let mem = Lengths::parse_records().parse_next(&mut ParseBytes {
        input: LocatingSlice::new(short),
        state: ParseContext::<()>::default(),
    });
    let mut ctx = ParseContext::<()>::default();
    let got = Lengths::parse_records_reader(Trickle::new(short, 2), &mut ctx, reading(1, BIG));
    agree("cut short", &mem, &got, false);
}

#[test]
fn invalid_utf8_in_a_text_stream_is_an_error_at_its_offset() {
    let mut input = b"abc abc ".to_vec();
    input.push(0xff);
    input.extend(b" abc");
    let mut ctx = ParseContext::<()>::default();
    let e =
        Leaves::parse_file_reader(Trickle::new(&input, 1), &mut ctx, reading(1, BIG)).unwrap_err();
    assert_eq!(e.offset, 8);
    assert_eq!(e.headline(), "invalid UTF-8 at byte 8: 0xff");
    // A character cut by a read is not an error.
    let mut ctx = ParseContext::<()>::default();
    Leaves::parse_file_reader(Trickle::new("éé".as_bytes(), 1), &mut ctx, reading(1, BIG))
        .unwrap_err();
}

// --- One built-in per grammar, so that its own lookahead is the grammar's --

// --- One built-in per grammar, so that its own lookahead is the grammar's --

mod single_f64 {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<u64> = v:fold(item, Vec::new, |mut v: Vec<u64>, x: u64| { v.push(x); v }) -> { v }
            item -> u64 = x:f64 ";" -> { x.to_bits() }
        }
    }
    pub const PIECES: &[&str] = &[
        r#"1.5"#,
        r#";"#,
        r#"1e3"#,
        r#"1e"#,
        r#"-"#,
        r#"inf"#,
        r#"infinity"#,
        r#"nan"#,
        r#"1."#,
        r#"2"#,
        r#" "#,
    ];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("f64: {input:?}, step {step}"), &mem, &got, false);
        }
    }
}

mod single_bool {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<bool> = v:fold(item, Vec::new, |mut v: Vec<bool>, x: bool| { v.push(x); v }) -> { v }
            item -> bool = x:bool ";" -> { x }
        }
    }
    pub const PIECES: &[&str] = &[
        r#"true"#, r#"false"#, r#"fals"#, r#"tru"#, r#";"#, r#"t"#, r#" "#,
    ];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("bool: {input:?}, step {step}"), &mem, &got, false);
        }
    }
}

mod single_char {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<char> = v:fold(item, Vec::new, |mut v: Vec<char>, x: char| { v.push(x); v }) -> { v }
            item -> char = x:char ";" -> { x }
        }
    }
    pub const PIECES: &[&str] = &[r#"'a'"#, r#"'\n'"#, r#"'"#, r#"'é'"#, r#"a"#, r#";"#];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("char: {input:?}, step {step}"), &mem, &got, false);
        }
    }
}

mod single_string {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<String> = v:fold(item, Vec::new, |mut v: Vec<String>, x: String| { v.push(x); v }) -> { v }
            item -> String = x:string ";" -> { x.to_string() }
        }
    }
    pub const PIECES: &[&str] = &[r#""ab""#, r#""a\"b""#, r#"""#, r#"\"#, r#";"#, r#"a"#];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(
                &format!("string: {input:?}, step {step}"),
                &mem,
                &got,
                false,
            );
        }
    }
}

mod single_i64 {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<i64> = v:fold(item, Vec::new, |mut v: Vec<i64>, x: i64| { v.push(x); v }) -> { v }
            item -> i64 = x:i64 ";" -> { x }
        }
    }
    pub const PIECES: &[&str] = &[
        r#"12"#,
        r#"-3"#,
        r#"-"#,
        r#"+4"#,
        r#"9999999999999999999"#,
        r#";"#,
    ];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("i64: {input:?}, step {step}"), &mem, &got, false);
        }
    }
}

mod single_raw_ident {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<String> = v:fold(item, Vec::new, |mut v: Vec<String>, x: String| { v.push(x); v }) -> { v }
            item -> String = x:raw_ident ";" -> { x.to_string() }
        }
    }
    pub const PIECES: &[&str] = &[r#"ab"#, r#"é"#, r#"日本"#, r#"_"#, r#";"#, r#"1"#];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(
                &format!("raw_ident: {input:?}, step {step}"),
                &mem,
                &got,
                false,
            );
        }
    }
}

mod single_any {
    use super::*;
    grammar! {
        grammar G {
            #[stream]
            pub list -> Vec<char> = v:fold(item, Vec::new, |mut v: Vec<char>, x: char| { v.push(x); v }) -> { v }
            item -> char = x:any ";" -> { x }
        }
    }
    pub const PIECES: &[&str] = &[r#"a"#, r#"é"#, r#"日"#, r#"𝄞"#, r#";"#];
    pub fn check(input: &'static str) {
        let mem = mem_text(&mut G::parse_list(), input, Diagnose::default());
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(&format!("any: {input:?}, step {step}"), &mem, &got, false);
        }
    }
}

#[test]
fn each_built_in_alone_agrees_with_the_parse_in_memory() {
    for input in pieces(0x1b, 400, single_f64::PIECES, 6) {
        single_f64::check(leak(input));
    }
    for input in pieces(0x2b, 400, single_bool::PIECES, 6) {
        single_bool::check(leak(input));
    }
    for input in pieces(0x3b, 400, single_char::PIECES, 6) {
        single_char::check(leak(input));
    }
    for input in pieces(0x4b, 400, single_string::PIECES, 6) {
        single_string::check(leak(input));
    }
    for input in pieces(0x5b, 400, single_i64::PIECES, 6) {
        single_i64::check(leak(input));
    }
    for input in pieces(0x6b, 400, single_raw_ident::PIECES, 6) {
        single_raw_ident::check(leak(input));
    }
    for input in pieces(0x7b, 400, single_any::PIECES, 6) {
        single_any::check(leak(input));
    }
}

mod single_any_bytes {
    use super::*;
    grammar! {
        grammar G {
            input bytes;
            #[stream]
            pub list -> Vec<char> = v:fold(item, Vec::new, |mut v: Vec<char>, x: char| { v.push(x); v }) -> { v }
            item -> char = x:any ";" -> { x }
        }
    }
    pub const PIECES: &[&str] = &["a", "é", "日", "𝄞", ";"];
    pub fn check(input: &'static str) {
        let mem = G::parse_list().parse_next(&mut ParseBytes {
            input: LocatingSlice::new(input.as_bytes()),
            state: ParseContext::<()>::default(),
        });
        for step in [1, 2] {
            let mut ctx = ParseContext::<()>::default();
            let got = G::parse_list_reader(
                Trickle::new(input.as_bytes(), step),
                &mut ctx,
                reading(1, BIG),
            );
            agree(
                &format!("any over bytes: {input:?}, step {step}"),
                &mem,
                &got,
                false,
            );
        }
    }
}

#[test]
fn a_character_decoded_from_bytes_waits_for_all_of_its_bytes() {
    for input in pieces(0xb17e, 400, single_any_bytes::PIECES, 6) {
        single_any_bytes::check(leak(input));
    }
}
