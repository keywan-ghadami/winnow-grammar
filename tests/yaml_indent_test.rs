//! YAML's block structure, written as the specification writes it: the
//! indentation `n` and the context `c` are parameters of the productions
//! (ADR 26). A subset - block sequences, block mappings, plain one-line
//! scalars, compact nested sequences (`- - a`) - but with the parts that make
//! YAML context-sensitive:
//!
//! * a block's indentation is detected from its first line, and must exceed
//!   its parent's (`m > n`);
//! * every further entry is at exactly that indentation (`" "{m}`);
//! * a sequence that is a mapping's value may sit at the mapping's own
//!   indentation (the spec's `seq-spaces(n, block-out)`), and nowhere else;
//! * a compact sequence continues at the column its first entry began in
//!   (`n + 1 + m`).

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

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

grammar! {
    grammar Yml {
        use super::{Ctx, Yaml};

        // Whitespace is structure here: nothing is skipped.
        WS -> () = "" -> { () }

        // The document is a block at indentation -1: any indentation exceeds it.
        pub DOC -> Yaml = v:BLOCK(n = -1, c = Ctx::BlockIn) eof -> { v }

        // A block on the lines below its parent, at the indentation of its
        // first line - which must exceed the parent's. A sequence that is a
        // mapping's value may also sit at the mapping's own indentation.
        BLOCK(n: isize, c: Ctx) -> Yaml =
            m:count(" ") if((m as isize) > n) v:CONTENT(m = m as isize) -> { v }
          | m:count(" ") if(c == Ctx::BlockOut && (m as isize) == n) s:SEQ(m = n) -> { Yaml::Seq(s) }

        // At column `m`, on the first line of a node.
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

        // What follows `-` or `key:`: a block on the lines below, or the
        // rest of this line. After `-` that may be a compact node, whose
        // further entries are at the column it began in: `- - a`.
        AFTER(m: isize, c: Ctx) -> Yaml =
            "\n" v:BLOCK(n = m, c = c) -> { v }
          | s:count(" ") if(s > 0 && c == Ctx::BlockIn, "a sequence entry")
            v:CONTENT(m = m + 1 + s as isize) -> { v }
          | s:count(" ") if(s > 0) v:SCALAR "\n" -> { Yaml::Scalar(v.to_string()) }

        KEY -> &'a str = s:until(":" | "\n") if(!s.is_empty() && !s.starts_with(' ')) -> { s }
        SCALAR -> &'a str = s:until("\n") if(!s.is_empty()) -> { s }
    }
}

fn s(v: &str) -> Yaml {
    Yaml::Scalar(v.into())
}

fn map(entries: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

#[test]
fn nested_mappings_and_sequences() {
    let doc = "\
name: winnow
tags:
  - parser
  - grammar
owner:
  login: keywan
  repos:
    - a
    - b
";
    Yml::parse_DOC().parse_test(doc).assert_success_is(map(vec![
        ("name", s("winnow")),
        ("tags", Yaml::Seq(vec![s("parser"), s("grammar")])),
        (
            "owner",
            map(vec![
                ("login", s("keywan")),
                ("repos", Yaml::Seq(vec![s("a"), s("b")])),
            ]),
        ),
    ]));
}

#[test]
fn the_indentation_is_the_first_lines() {
    // Four spaces as well as two: what counts is that all entries agree.
    let doc = "a:\n    - x\n    - y\nb: z\n";
    Yml::parse_DOC().parse_test(doc).assert_success_is(map(vec![
        ("a", Yaml::Seq(vec![s("x"), s("y")])),
        ("b", s("z")),
    ]));
    // An entry that does not agree is not one of them.
    Yml::parse_DOC()
        .parse_test("a:\n    - x\n   - y\n")
        .assert_failure();
    Yml::parse_DOC()
        .parse_test("a:\n  b: 1\n   c: 2\n")
        .assert_failure();
}

#[test]
fn a_mappings_sequence_may_sit_at_its_own_indentation() {
    // `seq-spaces(n, block-out)`: a mapping's value...
    let doc = "a:\n- x\n- y\nb: z\n";
    Yml::parse_DOC().parse_test(doc).assert_success_is(map(vec![
        ("a", Yaml::Seq(vec![s("x"), s("y")])),
        ("b", s("z")),
    ]));
    // ...but a mapping there is no value of `a`: a key at the same
    // indentation is the next key.
    Yml::parse_DOC().parse_test("a:\nb: z\n").assert_failure();
    // And a sequence entry's content is in block-in: no exception there.
    Yml::parse_DOC().parse_test("-\n- x\n").assert_failure();
}

#[test]
fn a_compact_sequence_continues_where_it_began() {
    let doc = "- - a\n  - b\n- c\n";
    Yml::parse_DOC()
        .parse_test(doc)
        .assert_success_is(Yaml::Seq(vec![Yaml::Seq(vec![s("a"), s("b")]), s("c")]));
    // Three spaces after the dash: the inner entries are at column 4.
    let doc = "-   - a\n    - b\n";
    Yml::parse_DOC()
        .parse_test(doc)
        .assert_success_is(Yaml::Seq(vec![Yaml::Seq(vec![s("a"), s("b")])]));
    // A compact mapping in a sequence entry, too.
    let doc = "- k: v\n  l: w\n";
    Yml::parse_DOC()
        .parse_test(doc)
        .assert_success_is(Yaml::Seq(vec![map(vec![("k", s("v")), ("l", s("w"))])]));
}

#[test]
fn a_mapping_value_is_no_compact_node() {
    // `a: - x` is a scalar `- x`, not a sequence: compact nodes are for
    // sequence entries (block-in) only.
    Yml::parse_DOC()
        .parse_test("a: - x\n")
        .assert_success_is(map(vec![("a", s("- x"))]));
}
