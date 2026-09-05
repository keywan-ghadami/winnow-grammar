# ADR 15: The Error Message Contract

**Status:** Accepted. **Date:** 2026-09-01.
**Tests:** `tests/diagnostics.rs`, one test per point. Where the two disagree, this ADR wins.

## Context

winnow-grammar is meant to be the front end for transpilers to Rust. There the
error message *is* the product: whoever translates a foreign language gets
errors in *that* language and has to understand them without knowing the
grammar.

Until now the error type came from winnow (`ContextError`). Measured on
`fn f(a: );` with `ty = ident | "&" ident`:

```
invalid ty
expected `&`
```

Four structural gaps: `ContextError::or` simply returns the *later* error, so
the last alternative (`&`) won and `ident` vanished; only one label (the
innermost), no rule stack; no view past a successful backtrack (`x?` and `x*`
threw their reason away); and what was actually found was never named.
Variant labels (`# "…"`) were parsed and then ignored by the code generator.

syn-grammar has an engine for this (ADR 13 there). It was **easier** to build
here: in text, progress is a byte offset that `LocatingSlice` provides for
free — the cursor trick is not needed.

## Decision

A dedicated error type, `winnow_grammar::ParseError`, with `offset`,
`expected`, `message`, `found`, `rule_stack` and `priority`. It implements
winnow's `ParserError`, so `alt` passes it through `or` — and `or` **is** the
selection:

1. **Progress**: the error with the larger offset wins — even against a
   `fail(..)` that stood earlier.
2. **Priority** at the same position: `fail` (50) > aggregation (20) >
   label (10) > default (0).
3. **Aggregation** on a tie: the expectations are merged.

What `alt` never sees — errors that `x?` and `x*` discard on a *successful*
backtrack — is remembered in `ParseContext::furthest`; `rt::finish` weighs
it against the returned error at the end. A remembered error gets its outer
rules from the **live** rule stack (`ParseContext::rules`), because it never
travels the return path where rules are normally collected.

The error is boxed (`ParseError(Box<ErrorCore>)`, fields reachable through `Deref`):
every closure level of a generated parser holds a `Result<_, ErrMode<ParseError>>`
on the stack, and with the payload inline (about 130 bytes instead of 32) a rule
nested 500 deep overflowed the stack in a debug build.

## The contract

| # | Promise | Example |
|---|---|---|
| 1 | Alternatives failing at the same position are merged, and what was found is named | ``expected one of: `&`, identifier; found unexpected token `)` `` |
| 2 | Position as line and column, 1-based | `at line 2, column 8` |
| 3 | Rule stack, innermost first — also for remembered errors | `in ty / in arg / in item 1 / in decl` |
| 4 | End of input is called by name | ``unexpected end of input, expected `;` `` |
| 5 | Trailing input reports the reason, not just "expected end of input" | ``expected `;`; found unexpected token `extra` `` |
| 6 | A labelled alternative (`# "…"`) contributes its name at its boundary | `expected one of: number, string` |
| 7 | … but only without progress; otherwise its own message stays | ``expected `"` `` |
| 8 | `fail("…")` wins at the same position | `custom failure here` |
| 9 | Progress beats `fail` | ``expected `b` `` instead of the `fail` text |
| 10 | List items carry their index | `in item 2` |
| 11 | `Display` without position, `render(source)` with it | winnow's `Parser::parse` prints the position itself |
| 12 | The error is a value with fields | `e.expected`, `e.found`, `e.rule_stack`, `e.offset` |

Built-ins get an expectation (`identifier`, `integer literal`, …) because
winnow's primitives only report the position.

## Consequences

* **Breaking:** `parse_<rule>()` returns `winnow_grammar::ParseError` instead
  of `ContextError`. The message text changes: `invalid X` becomes
  ``expected …; found unexpected token `…` `` plus `in X` lines. Hand-written
  parsers plugged into a grammar must return `ErrMode<ParseError>`;
  `ParseError` implements `FromExternalError` for `parse_to()` and
  `AddContext<StrContext>`, so winnow combinators keep working unchanged.
* `ParseContext` has two new fields (`furthest`, `rules`). Code that builds it
  through `Default` is unaffected.
* Still missing compared to syn-grammar: `recover(..)` does not report the
  error it skipped over (it is discarded, not remembered), and `until` has no
  expectation. Both are additions, not rebuilds.
