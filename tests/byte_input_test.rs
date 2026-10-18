//! A grammar over bytes: `input bytes;` (ADR 25).
//!
//! What the tests establish: a byte grammar takes input a text grammar cannot
//! be given at all (a binary record); on text it parses what the text grammar
//! parses, to the same result; what it matches by comparison is not checked,
//! and what becomes text is - `text(p)`, `intern(p)`, `raw_ident` - with a bad
//! byte reported as a cut at its absolute offset, `invalid UTF-8 at byte N:
//! 0xXX`; `intern(p)` checks a name only the first time it meets it; a
//! declared interner without `lookup_bytes` still works; a `par_fold` over
//! bytes in pieces agrees with the sequential parse.

use std::collections::HashMap;
use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::rt::Parallelism;
use winnow_grammar::{
    grammar, Diagnose, Interner, InternerOf, ParseBytes, ParseContext, ParseError, ParseInput,
    Symbol,
};

/// Parses `input` with `p` over bytes, and renders a failure against the
/// bytes.
fn run<'a, O, P>(mut p: P, input: &'a [u8]) -> Result<O, String>
where
    P: Parser<ParseBytes<'a, ()>, O, ParseError>,
{
    run_in(&mut p, input, ParseContext::default()).map_err(|e| e.render(input))
}

fn run_in<'a, O, P, S: Clone + std::fmt::Debug>(
    p: &mut P,
    input: &'a [u8],
    state: ParseContext<S>,
) -> Result<O, ParseError>
where
    P: Parser<ParseBytes<'a, S>, O, ParseError>,
{
    let mut stream = ParseBytes {
        input: LocatingSlice::new(input),
        state,
    };
    p.parse_next(&mut stream)
}

fn run_text<'a, O, P>(mut p: P, input: &'a str) -> Result<O, String>
where
    P: Parser<ParseInput<'a, ()>, O, ParseError>,
{
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::default(),
    };
    p.parse_next(&mut stream).map_err(|e| e.render(input))
}

// --- A binary format -------------------------------------------------------

/// A field of the record: one length byte, then that many bytes of payload.
/// What no grammar of literals and classes can say - the length is data.
fn field<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseBytes<'a, S>,
) -> Result<&'a [u8], ParseError> {
    let n = winnow::token::any::<_, ParseError>.parse_next(i)?;
    if i.eof_offset() < n as usize {
        return Err(ParseError::from_input(i).add_expected("the field's payload"));
    }
    Ok(i.next_slice(n as usize))
}

grammar! {
    grammar Records {
        input bytes;
        extern rule field -> &'a [u8];

        // `name:payload` records, `\n` after each. The name is text, the
        // payload arbitrary bytes - `\xff`, `\n` and `:` among them.
        pub RECORDS -> Vec<(&'a str, &'a [u8])> = rs:RECORD* -> { rs }
        pub RECORD -> (&'a str, &'a [u8]) = n:text(until(":")) ":" f:field "\n" -> { (n, f) }

        // What a literal, `until` and `bytes(p)` yield over bytes.
        pub RAW -> (&'a [u8], &'a [u8], &'a [u8]) =
            a:"<" b:bytes(digit1 "-" any) c:until(">") ">" -> { (a, b, c) }

        // `any` is a byte in a byte grammar.
        pub BYTE -> u8 = "#" b:any -> { b }
    }
}

#[test]
fn a_binary_record_parses() {
    let mut input = Vec::new();
    input.extend_from_slice(b"alpha:");
    input.push(4);
    input.extend_from_slice(&[0xff, b'\n', b':', 0x00]);
    input.push(b'\n');
    input.extend_from_slice("bärlauch:".as_bytes());
    input.push(0);
    input.push(b'\n');

    let rs = run(Records::parse_RECORDS(), &input).unwrap();
    assert_eq!(
        rs,
        vec![
            ("alpha", &[0xff, b'\n', b':', 0x00][..]),
            ("bärlauch", &[][..])
        ]
    );
    // The same bytes cannot even be handed to a text grammar.
    assert!(std::str::from_utf8(&input).is_err());
}

#[test]
fn literals_until_and_bytes_yield_bytes() {
    let (a, b, c) = run(Records::parse_RAW(), b"<12-\xfe\xff\x00>").unwrap();
    assert_eq!(a, b"<");
    assert_eq!(b, b"12-\xfe");
    assert_eq!(c, b"\xff\x00");
    assert_eq!(run(Records::parse_BYTE(), b"#\xc3").unwrap(), 0xc3);
}

#[test]
fn a_payload_too_short_is_an_ordinary_error() {
    let err = run(Records::parse_RECORD(), b"a:\x05ab\n").unwrap_err();
    assert!(err.contains("the field's payload"), "{err}");
}

// --- The 1BRC shape, over bytes and over text ------------------------------

grammar! {
    grammar BrcBytes {
        input bytes;
        #[frame(boundary = "\n")]
        pub ROW -> (&'a str, i32) =
            n:text(until(";" | frame_end)) ";" neg:"-"? w:dec<i32>(digit{1,2}) "." f:digit "\n" -> {
                let t = w * 10 + (f as i32 - '0' as i32);
                (n, if neg.is_some() { -t } else { t })
            }
        pub FILE -> HashMap<&'a str, (i32, i32, i64, u32)> = m:par_fold(
            ROW,
            HashMap::new,
            |acc: &mut HashMap<&'a str, (i32, i32, i64, u32)>, (n, t): (&'a str, i32)| {
                let e = acc.entry(n).or_insert((i32::MAX, i32::MIN, 0, 0));
                e.0 = e.0.min(t);
                e.1 = e.1.max(t);
                e.2 += t as i64;
                e.3 += 1;
            },
            |mut a: HashMap<&'a str, (i32, i32, i64, u32)>, b: HashMap<&'a str, (i32, i32, i64, u32)>| {
                for (k, v) in b {
                    let e = a.entry(k).or_insert((i32::MAX, i32::MIN, 0, 0));
                    e.0 = e.0.min(v.0);
                    e.1 = e.1.max(v.1);
                    e.2 += v.2;
                    e.3 += v.3;
                }
                a
            }
        ) -> { m }

        // Interned names, as Nikaia's program keeps them.
        pub NAMES -> Vec<Symbol> = ns:NAME* -> { ns }
        pub NAME -> Symbol = n:intern(until(";")) ";" digit{1,2} "\n" -> { n }
    }
}

grammar! {
    grammar BrcText {
        #[frame(boundary = "\n")]
        pub ROW -> (&'a str, i32) =
            n:text(until(";" | frame_end)) ";" neg:"-"? w:dec<i32>(digit{1,2}) "." f:digit "\n" -> {
                let t = w * 10 + (f as i32 - '0' as i32);
                (n, if neg.is_some() { -t } else { t })
            }
        pub FILE -> HashMap<&'a str, (i32, i32, i64, u32)> = m:par_fold(
            ROW,
            HashMap::new,
            |acc: &mut HashMap<&'a str, (i32, i32, i64, u32)>, (n, t): (&'a str, i32)| {
                let e = acc.entry(n).or_insert((i32::MAX, i32::MIN, 0, 0));
                e.0 = e.0.min(t);
                e.1 = e.1.max(t);
                e.2 += t as i64;
                e.3 += 1;
            },
            |mut a: HashMap<&'a str, (i32, i32, i64, u32)>, b: HashMap<&'a str, (i32, i32, i64, u32)>| {
                for (k, v) in b {
                    let e = a.entry(k).or_insert((i32::MAX, i32::MIN, 0, 0));
                    e.0 = e.0.min(v.0);
                    e.1 = e.1.max(v.1);
                    e.2 += v.2;
                    e.3 += v.3;
                }
                a
            }
        ) -> { m }
    }
}

/// A 1BRC-style input: `rows` rows over a few stations, names with and
/// without non-ASCII characters.
fn brc(rows: usize) -> String {
    let stations = [
        "Hamburg",
        "Bulawayo",
        "Palembang",
        "St. John's",
        "Cracóvia",
        "İzmir",
        "Øslo",
    ];
    let mut s = String::new();
    let mut x: u32 = 7;
    for i in 0..rows {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let t = (x >> 8) as i32 % 999;
        let t = if i % 3 == 0 { -t } else { t };
        s.push_str(stations[(x >> 3) as usize % stations.len()]);
        s.push(';');
        if t < 0 {
            s.push('-');
        }
        s.push_str(&format!("{}.{}\n", t.abs() / 10, t.abs() % 10));
    }
    s
}

#[test]
fn the_1brc_shape_agrees_with_the_text_grammar() {
    let input = brc(500);
    let over_bytes = run(BrcBytes::parse_FILE(), input.as_bytes()).unwrap();
    let over_text = run_text(BrcText::parse_FILE(), &input).unwrap();
    assert_eq!(over_bytes, over_text);
    assert_eq!(over_bytes.values().map(|v| v.3).sum::<u32>(), 500);
}

#[test]
fn rows_agree_with_the_text_grammar_including_their_errors() {
    for row in [
        "Hamburg;12.3\n",
        "Hamburg;-1.0\n",
        "Øslo;99.9\n",
        "x;123.4\n",
        "x;1.\n",
        ";1.0\n",
        "Hamburg;1.0",
        "Hamburg",
    ] {
        let b = run(BrcBytes::parse_ROW(), row.as_bytes());
        let t = run_text(BrcText::parse_ROW(), row);
        assert_eq!(b, t, "{row:?}");
    }
}

#[test]
fn par_fold_in_pieces_agrees_with_the_sequential_parse() {
    let input = brc(2_000);
    let ctx = ParseContext::<()>::default();
    let sequential = run(BrcBytes::parse_FILE(), input.as_bytes()).unwrap();
    for how in [
        Parallelism::Off,
        Parallelism::Pieces(1),
        Parallelism::Pieces(3),
        Parallelism::Pieces(7),
        Parallelism::Pieces(64),
        Parallelism::Auto,
    ] {
        let pieces = BrcBytes::parse_FILE_pieces(input.as_bytes(), &ctx, how).unwrap();
        assert_eq!(pieces, sequential, "{how:?}");
    }
    // The cut is the byte split, and the same as the text grammar's.
    assert_eq!(
        BrcBytes::frames_ROW(input.as_bytes(), 5),
        BrcText::frames_ROW(&input, 5)
    );
}

#[test]
fn a_bad_byte_in_a_piece_is_reported_at_its_place_in_the_whole_input() {
    let mut input = brc(400).into_bytes();
    // A name in the second half, broken.
    let at = input.len() - 40;
    let at = at + input[at..].iter().position(|&b| b == b'\n').unwrap() + 1;
    input[at] = 0xff;
    let expected = format!("invalid UTF-8 at byte {at}: 0xff");
    let sequential = run_in(
        &mut BrcBytes::parse_FILE(),
        &input,
        ParseContext::<()>::default(),
    )
    .unwrap_err();
    assert_eq!(sequential.offset, at);
    assert_eq!(sequential.headline(), expected);
    for n in [2, 5, 16] {
        let ctx = ParseContext::<()>::default();
        let e = BrcBytes::parse_FILE_pieces(&input, &ctx, Parallelism::Pieces(n)).unwrap_err();
        assert_eq!(e.offset, at, "{n} pieces");
        assert_eq!(e.headline(), expected, "{n} pieces");
    }
}

// --- What becomes text is checked ------------------------------------------

grammar! {
    grammar Names {
        input bytes;
        pub TEXT -> &'a str = t:text(until(";")) ";" -> { t }
        pub INTERN -> Symbol = s:intern(until(";")) ";" -> { s }
        pub IDENT -> &'a str = i:raw_ident ";" -> { i }
        // A bad byte is a cut: the second alternative, which would read the
        // same bytes as bytes, is not tried.
        pub EITHER -> usize = t:text(until(";")) ";" -> { t.len() } | b:bytes(until(";")) ";" -> { b.len() }
        pub rule words -> Vec<&'a str> = ws:raw_ident* -> { ws }
        pub STRING -> &'a str = s:string -> { s }
        pub CHAR -> char = c:char -> { c }
        pub DIGITS -> &'a str = d:digit{1,3} -> { d }
        pub TWO -> () = TEXT "\n" TEXT -> { () }
    }
}

/// `input` with byte `bad` at `at`.
fn broken(input: &str, at: usize, bad: &[u8]) -> Vec<u8> {
    let mut b = input.as_bytes().to_vec();
    b.splice(at..at, bad.iter().copied());
    b
}

fn assert_cut_at<O: std::fmt::Debug>(r: Result<O, ParseError>, at: usize, byte: u8) {
    let e = r.expect_err("a bad byte is an error");
    assert_eq!(e.offset, at);
    assert_eq!(
        e.headline(),
        format!("invalid UTF-8 at byte {at}: 0x{byte:02x}")
    );
}

#[test]
fn text_intern_and_raw_ident_reject_a_bad_byte_where_it_is() {
    // First, middle and last byte of a name, then a truncated multi-byte
    // character at the end of the input.
    let cases: [(Vec<u8>, usize, u8); 4] = [
        (broken("abc;", 0, b"\xff"), 0, 0xff),
        (broken("abc;", 1, b"\xc0"), 1, 0xc0),
        (broken("abc;", 3, b"\xfe"), 3, 0xfe),
        (b"ab\xc3".to_vec(), 2, 0xc3),
    ];
    for (input, at, byte) in &cases {
        assert_cut_at(
            run_in(
                &mut Names::parse_TEXT(),
                input,
                ParseContext::<()>::default(),
            ),
            *at,
            *byte,
        );
        assert_cut_at(
            run_in(
                &mut Names::parse_INTERN(),
                input,
                ParseContext::<()>::default(),
            ),
            *at,
            *byte,
        );
        assert_cut_at(
            run_in(
                &mut Names::parse_IDENT(),
                input,
                ParseContext::<()>::default(),
            ),
            *at,
            *byte,
        );
    }
}

#[test]
fn a_bad_byte_after_a_prefix_is_reported_at_its_absolute_offset() {
    // `words` reads two identifiers and breaks in the third: the offset is
    // the byte's in the whole input, not in the identifier.
    let input = b"alpha beta g\xe2\x28\xa1mma";
    assert_cut_at(
        run_in(
            &mut Names::parse_words(),
            input,
            ParseContext::<()>::default(),
        ),
        12,
        0xe2,
    );
    // Valid wide characters are decoded and kept.
    assert_eq!(
        run(Names::parse_words(), "größe ÿ".as_bytes()).unwrap(),
        vec!["größe", "ÿ"]
    );
}

#[test]
fn a_bad_byte_is_a_cut_not_a_backtrack() {
    let input = broken("abc;", 1, b"\xff");
    assert_cut_at(
        run_in(
            &mut Names::parse_EITHER(),
            &input,
            ParseContext::<()>::default(),
        ),
        1,
        0xff,
    );
    assert_eq!(run(Names::parse_EITHER(), b"abc;").unwrap(), 3);
}

#[test]
fn the_bad_byte_is_found_by_both_passes_and_every_diagnose_mode() {
    let input = broken("abc;", 2, b"\xff");
    for diagnose in [Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Eager] {
        let ctx = ParseContext::<()> {
            diagnose,
            ..Default::default()
        };
        assert_cut_at(run_in(&mut Names::parse_TEXT(), &input, ctx), 2, 0xff);
    }
    let ctx = ParseContext::<()> {
        diagnose: Diagnose::Off,
        ..Default::default()
    };
    assert!(run_in(&mut Names::parse_TEXT(), &input, ctx)
        .unwrap_err()
        .is_undiagnosed());
}

#[test]
fn string_and_char_are_decoded_with_validation() {
    assert_eq!(
        run(Names::parse_STRING(), "\"grüße\"".as_bytes()).unwrap(),
        "grüße"
    );
    assert_cut_at(
        run_in(
            &mut Names::parse_STRING(),
            b"\"gr\xffe\"",
            ParseContext::<()>::default(),
        ),
        3,
        0xff,
    );
    assert_eq!(run(Names::parse_CHAR(), "'ß'".as_bytes()).unwrap(), 'ß');
    assert_eq!(run(Names::parse_CHAR(), b"'\\n'").unwrap(), '\n');
    assert_cut_at(
        run_in(
            &mut Names::parse_CHAR(),
            b"'\xe2\x82'",
            ParseContext::<()>::default(),
        ),
        1,
        0xe2,
    );
}

#[test]
fn a_digit_run_is_text() {
    assert_eq!(run(Names::parse_DIGITS(), b"123").unwrap(), "123");
}

#[test]
fn the_rendering_shows_a_bad_byte_lossily() {
    let input = b"ok;\nab\xffc;";
    let e = run_in(
        &mut Names::parse_TWO(),
        input,
        ParseContext::<()>::default(),
    )
    .unwrap_err();
    assert_eq!(e.offset, 6);
    assert_eq!(
        e.render(&input[..]),
        "invalid UTF-8 at byte 6: 0xff at line 2, column 3\n   2 | ab\u{fffd}c;\n         ^\nin TEXT\nin TWO"
    );
    assert_eq!(e.line_column(&input[..]), (2, 3));
}

// --- Interning checks only on a miss ---------------------------------------

/// An interner that counts what it is asked to intern - which in a byte
/// grammar is exactly what was checked - and looks bytes up itself.
#[derive(Debug, Clone, Default)]
struct Counting {
    ids: HashMap<Vec<u8>, Symbol>,
    names: Vec<String>,
    interned: usize,
}

impl Interner for Counting {
    fn intern(&mut self, text: &str) -> Symbol {
        self.interned += 1;
        if let Some(&s) = self.ids.get(text.as_bytes()) {
            return s;
        }
        let s = Symbol::from_index(self.names.len() as u32);
        self.names.push(text.to_string());
        self.ids.insert(text.as_bytes().to_vec(), s);
        s
    }

    fn resolve(&self, symbol: Symbol) -> &str {
        &self.names[symbol.index() as usize]
    }

    fn lookup_bytes(&mut self, bytes: &[u8]) -> Option<Symbol> {
        self.ids.get(bytes).copied()
    }
}

/// An interner that says nothing about bytes: the trait's default.
#[derive(Debug, Clone, Default)]
struct Plain(Vec<String>);

impl Interner for Plain {
    fn intern(&mut self, text: &str) -> Symbol {
        if let Some(i) = self.0.iter().position(|n| n == text) {
            return Symbol::from_index(i as u32);
        }
        self.0.push(text.to_string());
        Symbol::from_index(self.0.len() as u32 - 1)
    }

    fn resolve(&self, symbol: Symbol) -> &str {
        &self.0[symbol.index() as usize]
    }
}

grammar! {
    grammar CountingNames {
        input bytes;
        interner Counting;
        pub NAMES -> Vec<Symbol> = ns:NAME* -> { ns }
        pub NAME -> Symbol = n:intern(until(";")) ";" -> { n }
    }
}

grammar! {
    grammar PlainNames {
        input bytes;
        interner Plain;
        pub NAMES -> Vec<Symbol> = ns:NAME* -> { ns }
        pub NAME -> Symbol = n:intern(until(";")) ";" -> { n }
    }
}

#[test]
fn a_name_the_interner_holds_is_not_checked_again() {
    let input = b"a;bb;a;a;bb;ccc;a;";
    let mut ctx = ParseContext::with_state(Counting::default());
    ctx.diagnose = Diagnose::Off;
    let mut stream = ParseBytes {
        input: LocatingSlice::new(&input[..]),
        state: ctx,
    };
    let ns = CountingNames::parse_NAMES()
        .parse_next(&mut stream)
        .unwrap();
    assert_eq!(ns.len(), 7);
    assert_eq!(ns[0], ns[2]);
    assert_eq!(ns[1], ns[4]);
    // Three names, three checks and three calls of `intern` - and a fourth
    // for the empty name the repetition tries at the end; the other four
    // occurrences were hits.
    let interner = &mut stream.state.user_state;
    assert_eq!(InternerOf::<Counting>::interner(interner).interned, 4);
}

#[test]
fn the_context_interner_checks_only_on_a_miss_and_agrees_on_symbols() {
    let input = b"Hamburg;1\nHamburg;2\nOslo;3\nHamburg;4\n";
    let ns = run(BrcBytes::parse_NAMES(), input).unwrap();
    assert_eq!(ns[0], ns[1]);
    assert_eq!(ns[0], ns[3]);
    assert_ne!(ns[0], ns[2]);
    // A hit on the cache is a name the interner holds - and the same symbol
    // as interning the text directly.
    let mut ctx = ParseContext::<()>::default();
    let direct = ctx.intern("Hamburg");
    assert_eq!(ctx.intern_bytes(b"Hamburg"), Ok(direct));
    assert_eq!(ctx.intern_bytes(b"Ham\xffburg"), Err(3));
}

#[test]
fn a_declared_interner_without_lookup_bytes_still_works_and_still_checks() {
    let input = b"x;y;x;";
    let mut ctx = ParseContext::with_state(Plain::default());
    let ns = run_in(&mut PlainNames::parse_NAMES(), input, ctx.clone()).unwrap();
    assert_eq!(ns[0], ns[2]);
    assert_ne!(ns[0], ns[1]);

    ctx.user_state = Plain::default();
    let bad = b"x;y\xff;";
    assert_cut_at(run_in(&mut PlainNames::parse_NAMES(), bad, ctx), 3, 0xff);
    // The same through `CountingNames`, whose own lookup misses first.
    let ctx = ParseContext::with_state(Counting::default());
    assert_cut_at(run_in(&mut CountingNames::parse_NAMES(), bad, ctx), 3, 0xff);
}
