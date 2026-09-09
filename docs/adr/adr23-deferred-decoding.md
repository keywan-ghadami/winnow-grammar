# ADR 23: Deferred Decoding - a Character Class Is Scanned as Bytes, a Word at a Time

**Status:** Accepted, implemented. **Date:** 2026-09-09.
**Tests:** `tests/ascii_scan_test.rs` - every class against its own byte
definition, for every byte value at every offset in a word; short inputs
through the tail; the boundary claim; and `raw_ident`'s Unicode continuation
against a character walk.
**Benchmarks:** `benches/deferred.rs`, `benches/interning.rs` (`parse/`).
**Related:** ADR 16 §4 (why cutting a `&str` at an ASCII match is safe),
ADR 21 (`par_fold` pieces), the byte driver (TODO §3), which this is *not*.

## Context

The 1BRC winners do not parse characters. They read bytes, compare eight at a
time in a register, and decode only what the answer needs. Two of those three
ideas apply to this crate today, without a byte grammar and without changing
what a rule means.

`digit1`, `alpha1`, `multispace0` and `raw_ident` are the generated parsers a
grammar spends its time in - the whitespace skip alone runs between every two
tokens. Each was `take_while` over a `char` predicate.

Measured first, because the obvious explanation was the wrong one:

| what | 200 000 bytes of digits |
|---|---|
| by `char` predicate (winnow's `take_while`) | 1.30 GiB/s |
| by byte, one at a time | 2.67 GiB/s |
| by word, eight at a time | **10.7 GiB/s** |

**Decoding was never the cost.** winnow already scans a `&str` byte by byte for
an ASCII predicate, so "stop decoding" buys nothing on its own; the by-char and
by-byte rows differ by the per-byte call, not by UTF-8. What buys the order of
magnitude is doing eight bytes in one comparison - and *that* is what needs the
input to be bytes, which is where deferred decoding earns its place.

## Decision

A character class the generator emits is a `crate::ascii::AsciiClass` - a list
of inclusive ASCII ranges - and is scanned by `rt::class` (or `rt::class_or_wide`)
eight bytes at a time. Nothing in the DSL changes: `digit1` is still `digit1`.

Three claims hold it up.

**1. The scan cannot stop inside a character.** A `&str` is valid UTF-8, and
every byte of a multi-byte character is `>= 0x80`. An ASCII class contains no
such byte, so the scan stops at or before the character's first byte, never
between its bytes. The slice it cuts is valid UTF-8 by construction - there is
nothing to validate and nothing to decode. This is ADR 16 §4's argument for
cutting frames, applied one level down.

**2. What actually needs a `char` gets one, there.** `raw_ident` matches
Unicode alphanumerics, so `über` is one identifier and an ASCII class alone
would stop at `ü`. `rt::class_or_wide` scans the ASCII stretch by word and
builds exactly one `char` when it meets a byte `>= 0x80`, then goes back to
scanning. Decoding is not removed; it is *deferred to the position that needs
it*, which is the whole idea and the reason this is not a byte grammar.

**3. The word test is checked against the byte definition, exhaustively.**
`AsciiClass::contains` is the definition, `run` is the optimisation, and
`tests/ascii_scan_test.rs` asserts they agree for every class, every byte
value, at every offset in a word. That test is not decoration: it caught a real
bug in the first implementation on its first run.

## The bug the exhaustive test caught

The first mask used Bit Twiddling Hacks' `hasless` / `hasmore`:

```rust
let below = w.wrapping_sub(splat(lo)) & !w;
let above = w.wrapping_add(splat(127 - hi)) | w;
```

Those are *contains* tests - "does this word hold a byte below n" - and the
`& !w` / `| w` guards make the aggregate answer right. They do not make each
byte's answer right, because a borrow out of a low byte still lands in the byte
above it: with `lo = 0x30`, a `0x00` in byte 0 borrows, and a `0x30` in byte 1 -
in the class - comes out as `0xFF`, high bit set, reported as out. Read as a
word the answer is fine; read as "where does the run stop" it is off by
whatever follows.

Every example anyone writes by hand passes. `"a1_ü2_東3!rest"` does not: the run
stopped after 2 bytes instead of 11.

The fix is to stop the borrow instead of guarding against it. Clear each byte's
high bit and set it again as a guard; every byte is then `>= 0x80`, larger than
any `n <= 0x80` being subtracted, so no borrow can cross a byte boundary and
the guard bit survives exactly when the byte was `>= n`:

```rust
let guarded = (w & !HIGH) | HIGH;
let ge_lo   = guarded.wrapping_sub(splat(lo)) & HIGH;
let ge_next = guarded.wrapping_sub(splat(hi + 1)) & HIGH;
inside |= ge_lo & !ge_next;
```

Bytes that had their high bit set to begin with are excluded at the end
(`inside & !w`), which is claim 1 in one operation. It costs two subtractions
per range where the buggy version cost one subtraction and one addition.

## Consequences

* A class scan is between as fast as before and ~8x faster, growing with the
  run length: +15% over a 2-byte run, +67% over 7, 2.5x over 20, 8.2x over
  200 000. A whitespace skip between two tokens is short and gains little, a
  long field or an indented block gains a lot; it is never slower. End to end,
  an identifier-heavy grammar parses ~10% faster (`parse/idents/2000`,
  -9.2% and -11.2% in two independent runs against the same baseline).
  `parse/rows` is unchanged, as it must be - that grammar overrides `WS` and
  uses no character class, so none of its code changed; the +3.9% its first
  run showed was noise, and re-runs gave -1.2% and -6.1%.
* `AsciiClass` is public (`winnow_grammar::ascii`), because a hand-written rule
  or an `extern` rule has the same reason to want it.
* Any new class must be expressible as ASCII ranges. A class that is not - a
  Unicode property, a user predicate - keeps `take_while`, and `class_or_wide`
  is the pattern for a class that is ASCII plus a decoded tail.
* This does **not** make the crate byte-oriented. The input is still `&str`,
  the rules still mean characters, and the byte driver (TODO §3) is still its
  own decision with its own ADR to write.
