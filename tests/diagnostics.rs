//! The contract for error messages - one test per point of ADR 15
//! (`docs/adr/adr15-diagnostics.md`). In case of conflict, the ADR wins.

use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stateful};
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseContext};

grammar! {
    grammar Diag {
        pub decl -> String = "fn" name:raw_ident "(" args:arg* ")" ";" -> { format!("{name}({})", args.join(",")) }
        arg -> String = n:raw_ident ":" t:typ ","? -> { format!("{n}:{t}") }
        typ -> String = t:raw_ident -> { t.to_string() } | "&" t:raw_ident -> { format!("&{t}") }

        // Labelled alternatives: at their boundary, the name counts as the expectation.
        pub assign -> String = "let" v:value ";" -> { v }
        value -> String = n:u32 # "number" -> { n.to_string() } | s:string # "string" -> { s.to_string() }

        // fail: high priority, but not fatal - progress takes precedence.
        pub guarded -> u32 = "a" fail("custom failure here") -> { 0 } | "a" "b" "c" -> { 1 }

        // A custom identifier rule that does not allow a leading digit -
        // already possible today, without changing `ident`.
        IDENT -> &'a str = not(digit1) s:raw_ident # "identifier" -> { s }
        pub named -> String = "fn" n:IDENT ";" -> { n.to_string() }
    }
}

fn fehler(q: &str) -> String {
    match Diag::parse_decl().parse_test(q).inner {
        Ok(v) => panic!("unerwartet erfolgreich: {v:?}"),
        Err(e) => e,
    }
}

/// Point 1: alternatives at the same position are aggregated - and what was
/// actually there is named.
#[test]
fn p01_alternativen_zusammengefasst_mit_fund() {
    let e = fehler("fn f(a: );");
    assert!(
        e.starts_with("expected one of: `&`, identifier; found unexpected token `)`"),
        "{e}"
    );
}

/// Point 2: position as line and column, 1-based, also across lines.
#[test]
fn p02_position_zeile_und_spalte() {
    assert!(
        fehler("fn f(a: );").contains(" at line 1, column 9"),
        "{}",
        fehler("fn f(a: );")
    );
    let e = fehler("fn f(\n    a: );");
    assert!(e.contains(" at line 2, column 8"), "{e}");
}

/// Point 3: the rule stack, innermost first - also for an error that a
/// successful backtrack (`arg*`) discarded. The outer rule `decl` comes from
/// the live stack.
#[test]
fn p03_regelstapel_innerste_zuerst() {
    let e = fehler("fn f(a: );");
    assert!(e.ends_with("\nin typ\nin arg\nin item 1\nin decl"), "{e}");
}

/// Point 4: at the end of input it is worded like this.
#[test]
fn p04_ende_der_eingabe() {
    let e = fehler("fn f(a: i32)");
    assert!(
        e.starts_with("unexpected end of input, expected `;`"),
        "{e}"
    );
}

/// Point 5: if input is left over, the message names the reason, not just
/// "expected end of input".
#[test]
fn p05_resteingabe_nennt_den_grund() {
    let e = fehler("fn f(a: i32) extra;");
    assert!(
        e.starts_with("expected `;`; found unexpected token `extra`"),
        "{e}"
    );
}

/// Point 6: a labelled alternative (`# "…"`) that fails at its boundary
/// contributes its name as the expectation - not its internal message.
#[test]
fn p06_label_als_erwartung() {
    let e = Diag::parse_assign().parse_test("let ?;").inner.unwrap_err();
    assert!(
        e.starts_with("expected one of: number, string; found unexpected token `?`"),
        "{e}"
    );
}

/// Point 7: if the labelled alternative made progress, its own message stays.
#[test]
fn p07_label_nur_ohne_fortschritt() {
    let e = Diag::parse_assign()
        .parse_test("let \"abc;")
        .inner
        .unwrap_err();
    assert!(!e.contains("expected one of: number, string"), "{e}");
    assert!(e.contains("expected `\"`"), "{e}");
}

/// Point 8: at the same position, `fail(..)` wins through its priority.
#[test]
fn p08_fail_gewinnt_bei_gleichstand() {
    let e = Diag::parse_guarded().parse_test("a").inner.unwrap_err();
    assert!(e.starts_with("custom failure here"), "{e}");
}

/// Point 9: ... but progress beats priority - an error that got further also
/// wins against a `fail(..)` that came earlier. (With `a c` both branches fail
/// at the same position, where `fail` rightly wins - see point 8. Here the
/// second branch gets past `b`.)
#[test]
fn p09_fortschritt_schlaegt_fail() {
    let e = Diag::parse_guarded().parse_test("a b x").inner.unwrap_err();
    assert!(
        e.starts_with("expected `c`; found unexpected token `x`"),
        "{e}"
    );
    assert!(!e.contains("custom failure"), "{e}");
}

/// Point 10: list elements carry their index, 1-based.
#[test]
fn p10_listenindex() {
    let e = fehler("fn f(a: i32, 123);");
    assert!(e.contains("\nin item 2\n"), "{e}");
}

/// Point 11: `Display` carries no position (winnow's `Parser::parse` prepends
/// it itself along with the source line); `render(source)` carries it.
#[test]
fn p11_display_ohne_position_render_mit() {
    let mut s = Stateful {
        state: ParseContext::<()>::default(),
        input: LocatingSlice::new("fn f(a: );"),
    };
    let e = Diag::parse_decl().parse_next(&mut s).unwrap_err();
    assert!(!e.to_string().contains("line"), "{e}");
    assert!(e.render("fn f(a: );").contains("at line 1, column 9"));
    assert_eq!(e.offset, 8);
}

/// Point 12: the error is a value with fields - tools can evaluate it instead
/// of parsing text.
#[test]
fn p12_fehler_ist_strukturiert() {
    let mut s = Stateful {
        state: ParseContext::<()>::default(),
        input: LocatingSlice::new("fn f(a: );"),
    };
    let e = Diag::parse_decl().parse_next(&mut s).unwrap_err();
    assert_eq!(
        e.expected,
        vec!["identifier".to_string(), "`&`".to_string()]
    );
    assert_eq!(e.found.as_deref(), Some(")"));
    assert_eq!(e.rule_stack, vec!["typ", "arg", "item 1", "decl"]);
}

/// A custom identifier rule without a leading digit already works today.
#[test]
fn eigene_ident_regel_ohne_fuehrende_ziffer() {
    Diag::parse_named()
        .parse_test("fn abc;")
        .assert_success_is("abc".to_string());
    let e = Diag::parse_named()
        .parse_test("fn 1abc;")
        .inner
        .unwrap_err();
    assert!(
        e.starts_with("expected identifier; found unexpected token `1abc`"),
        "{e}"
    );
}
