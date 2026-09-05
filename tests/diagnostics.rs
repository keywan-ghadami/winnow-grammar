//! Der Vertrag fuer Fehlermeldungen - ein Test je Punkt von ADR 15
//! (`docs/adr/adr15-diagnostics.md`). Bei Widerspruch gilt das ADR.

use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stateful};
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseContext};

grammar! {
    grammar Diag {
        pub decl -> String = "fn" name:raw_ident "(" args:arg* ")" ";" -> { format!("{name}({})", args.join(",")) }
        arg -> String = n:raw_ident ":" t:typ ","? -> { format!("{n}:{t}") }
        typ -> String = t:raw_ident -> { t.to_string() } | "&" t:raw_ident -> { format!("&{t}") }

        // Benannte Alternativen: an ihrer Grenze zaehlt der Name als Erwartung.
        pub assign -> String = "let" v:value ";" -> { v }
        value -> String = n:u32 # "number" -> { n.to_string() } | s:string # "string" -> { s.to_string() }

        // fail: hochprior, aber nicht fatal - Fortschritt geht vor.
        pub guarded -> u32 = "a" fail("custom failure here") -> { 0 } | "a" "b" "c" -> { 1 }

        // Eine eigene Bezeichnerregel, die keine Ziffer am Anfang zulaesst -
        // heute schon moeglich, ohne Aenderung an `ident`.
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

/// Punkt 1: Alternativen an derselben Stelle werden zusammengefasst - und
/// was tatsaechlich dastand, wird genannt.
#[test]
fn p01_alternativen_zusammengefasst_mit_fund() {
    let e = fehler("fn f(a: );");
    assert!(
        e.starts_with("expected one of: `&`, identifier; found unexpected token `)`"),
        "{e}"
    );
}

/// Punkt 2: Position als Zeile und Spalte, 1-basiert, auch ueber Zeilen hinweg.
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

/// Punkt 3: der Regelstapel, innerste zuerst - auch fuer einen Fehler, den
/// ein erfolgreiches Zuruecksetzen (`arg*`) verworfen hat. Die aeussere Regel
/// `decl` kommt vom lebenden Stapel.
#[test]
fn p03_regelstapel_innerste_zuerst() {
    let e = fehler("fn f(a: );");
    assert!(e.ends_with("\nin typ\nin arg\nin item 1\nin decl"), "{e}");
}

/// Punkt 4: am Ende der Eingabe heisst es so.
#[test]
fn p04_ende_der_eingabe() {
    let e = fehler("fn f(a: i32)");
    assert!(
        e.starts_with("unexpected end of input, expected `;`"),
        "{e}"
    );
}

/// Punkt 5: bleibt Eingabe uebrig, nennt die Meldung den Grund, nicht nur
/// "expected end of input".
#[test]
fn p05_resteingabe_nennt_den_grund() {
    let e = fehler("fn f(a: i32) extra;");
    assert!(
        e.starts_with("expected `;`; found unexpected token `extra`"),
        "{e}"
    );
}

/// Punkt 6: eine benannte Alternative (`# "…"`), die an ihrer Grenze scheitert,
/// steuert ihren Namen als Erwartung bei - nicht ihre interne Meldung.
#[test]
fn p06_label_als_erwartung() {
    let e = Diag::parse_assign().parse_test("let ?;").inner.unwrap_err();
    assert!(
        e.starts_with("expected one of: number, string; found unexpected token `?`"),
        "{e}"
    );
}

/// Punkt 7: kam die benannte Alternative voran, bleibt ihre eigene Meldung.
#[test]
fn p07_label_nur_ohne_fortschritt() {
    let e = Diag::parse_assign()
        .parse_test("let \"abc;")
        .inner
        .unwrap_err();
    assert!(!e.contains("expected one of: number, string"), "{e}");
    assert!(e.contains("expected `\"`"), "{e}");
}

/// Punkt 8: an derselben Stelle gewinnt `fail(..)` durch seine Prioritaet.
#[test]
fn p08_fail_gewinnt_bei_gleichstand() {
    let e = Diag::parse_guarded().parse_test("a").inner.unwrap_err();
    assert!(e.starts_with("custom failure here"), "{e}");
}

/// Punkt 9: ... aber Fortschritt schlaegt Prioritaet - ein weiter gekommener
/// Fehler gewinnt auch gegen ein `fail(..)`, das frueher stand. (Bei `a c`
/// scheitern beide Zweige an derselben Stelle, dort gewinnt `fail` zu Recht -
/// siehe Punkt 8. Hier kommt der zweite Zweig ueber `b` hinaus.)
#[test]
fn p09_fortschritt_schlaegt_fail() {
    let e = Diag::parse_guarded().parse_test("a b x").inner.unwrap_err();
    assert!(
        e.starts_with("expected `c`; found unexpected token `x`"),
        "{e}"
    );
    assert!(!e.contains("custom failure"), "{e}");
}

/// Punkt 10: Listenelemente tragen ihren Index, 1-basiert.
#[test]
fn p10_listenindex() {
    let e = fehler("fn f(a: i32, 123);");
    assert!(e.contains("\nin item 2\n"), "{e}");
}

/// Punkt 11: `Display` traegt keine Position (winnows `Parser::parse` stellt
/// sie samt Quellzeile selbst voran); `render(source)` traegt sie.
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

/// Punkt 12: der Fehler ist ein Wert mit Feldern - Werkzeuge koennen ihn
/// auswerten, statt Text zu parsen.
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

/// Eine eigene Bezeichnerregel ohne fuehrende Ziffer geht heute schon.
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
