# ADR 25: A Grammar Over Bytes - `input bytes;`, and Only What Becomes Text Is Checked

**Status:** Accepted, implemented (#24). **Date:** 2026-10-18.
**Tests:** `tests/byte_input_test.rs`, `tests/ui/input.rs`.
**Raised by:** TODO §3 ("The byte driver"), and
[Nikaia#422](https://github.com/Nikaia-Foundation/Nikaia/issues/422), which
needs a backend that reads bytes before Nikaia can decide what one of its views
is.
**Related:** ADR 23 (character classes are scanned as bytes), ADR 24 §4
(cutting a `&str` at an ASCII match is safe; `rt::frames_bytes`), ADR 18
(interning), ADR 15/17 (diagnostics).

## Context

Every generated parser takes `ParseInput<'a, S>`, a
`Stateful<LocatingSlice<&'a str>, ParseContext<S>>`. So a grammar can only be
run on text that someone has already proved to be UTF-8, in full, before the
first rule runs. Two kinds of input pay for that:

* **A format that is not text** - a binary record, a length-prefixed field, a
  file whose encoding is the grammar's business - cannot be described at all.
* **A text format read from untrusted bytes** pays one validating pass over the
  whole input and a second, parsing pass. Nikaia's 1BRC program does exactly
  this (`fs::map` checks the mapping, Nikaia ADR-016 D1). What it actually
  turns into text is a station name; the digits, the `;` and the `.` were
  compared byte for byte and could not have been anything but ASCII.

Byte input is what other parser tools treat as basic: `winnow` and `nom` are
generic over the stream and take `&[u8]` as readily as `&str`; Rust's `regex`
offers the same syntax again as `regex::bytes`; tree-sitter reads bytes. pest
is the one that is text only.

Most of this crate already works on bytes: `rt::frames_bytes` cuts `&[u8]` and
`rt::frames` is its `&str` view; `AsciiClass::run` takes a `&[u8]`; the error
engine takes `I: Stream + Location + AsBStr`. What is fixed to text is the
`ParseInput` alias, the runtime helpers that return `&'a str`, the built-ins,
`text(p)`, `intern(p)` and the diagnostic renderer's `source: &str`.

## Decision

### D1 - A grammar chooses its input once, with `input bytes;`

```rust,ignore
grammar! {
    grammar Records {
        input bytes;
        ...
    }
}
```

A declaration beside `state T;` and `interner I;`, at most once per grammar.
Without it a grammar is a text grammar, exactly as today: no signature, no
generated line and no built-in of an existing grammar changes.

With it, every rule takes `ParseBytes<'a, S>`, a
`Stateful<LocatingSlice<&'a [u8]>, ParseContext<S>>`, and the entry points
take `&'a [u8]` (`parse_<rule>`, `parse_<rule>_pieces`, `split_<rule>`). The
context, the state, the interner, the diagnose modes and the frames are the
same types as in a text grammar.

### D2 - What is matched by comparison is never checked

A literal, an ASCII character class (`digit`, `digit1`, `alpha1`, `hex_digit*`,
`space*`, `multispace*`, the whitespace skip), `dec<T>(p)`, a frame boundary and
`eof` compare bytes. Nothing they accept can be invalid UTF-8, so nothing about
them is checked, and they cost what they cost in a text grammar.

An ASCII class still yields `&'a str`, in both kinds of grammar: every byte it
accepted is below `0x80`, so the slice is valid UTF-8 by construction (the
argument of ADR 24 §4). That is the only place the byte driver turns bytes into
text without checking them, and the runtime function that does it says so in
its name and its safety comment.

### D3 - What becomes text is checked where it becomes text

In a byte grammar:

* `bytes(p)` runs `p` and yields the bytes it consumed, `&'a [u8]`, unchecked.
  It is new and exists in byte grammars only.
* `until(t)` yields `&'a [u8]`: what it skipped is whatever lay before `t`.
* `text(p)` runs `p` and checks the bytes it consumed as UTF-8, yielding
  `&'a str`. `text(until(";"))` is how a name becomes text.
* `intern(p)` checks as `text(p)` does, then interns (D4).
* A built-in that decodes characters (`raw_ident`, `ident`, `char`, the
  `lit_*` family, `string`) checks what it decodes, in the stretch it decodes:
  `raw_ident` already scans ASCII by word and decodes only from a byte
  `>= 0x80` (ADR 23); there it decodes with validation instead.
* `any` decodes one character with validation and yields `char`, as in a text
  grammar, so an action written for text compiles over bytes too; a bad or
  truncated sequence is the D5 cut. `byte` (byte grammars only) is the raw
  one, a `u8`, unchecked.

A text grammar keeps `until(t) -> &'a str` and has no `bytes(p)` or `byte`; its `text(p)`
does not check, because its input already was.

### D4 - A value the interner already holds is not checked again

`intern(p)` looks the bytes up first. A hit means the same bytes are already
held as a `&str`, so they are valid UTF-8 and the check is skipped; only a miss
is checked, once, before the text is stored. On 1BRC that is 413 checks for a
billion rows.

The `Interner` trait gains `lookup_bytes(&mut self, bytes: &[u8]) ->
Option<Symbol>` with a default that checks and calls the existing lookup, so a
user's declared interner keeps compiling and stays correct; the built-in
interners override it to look up without the check.

### D5 - A bad byte is a hard error at its own offset

A failed check is `ErrMode::Cut`, not a backtrack: no alternative that reads the
same bytes as text can succeed, and the error must name the byte, not the last
alternative tried. The error's offset is the absolute offset of the first byte
that is not UTF-8 (`Utf8Error::valid_up_to` from the start of the slice), and its
message says so: `invalid UTF-8 at byte 1043: 0xff`.

The diagnostic renderer takes the input as bytes. Line and column are counted
over bytes (`\n`); the excerpt it prints is shown lossily, so a bad byte
appears as `U+FFFD` under the caret instead of breaking the rendering. For a
text grammar the output is byte-identical to today's.

### D6 - Frames and `par_fold` carry over unchanged

`parse_<rule>_pieces` on a byte grammar cuts with `rt::frames_bytes` and runs
each piece as a `ParseBytes`. The frame check (ADR 24) is the same. A piece
never starts inside a multi-byte character, because a boundary is an ASCII byte
string; and if it did, the text a piece yields would still be checked (D3).

## Rejected

* **The input as a type parameter of the generated code**, beside `S` and `E`.
  It is the most general shape, and winnow's own. It puts a bound in every
  signature of every grammar, makes every built-in generic over a stream trait
  and every error message about one, and breaks every existing user, Nikaia's
  lowering among them - to buy inputs (`&[u16]`, a token stream) nobody has
  asked for. A third kind of input can be another declaration later.
* **A separate macro or crate for byte grammars** (`grammar_bytes!`). It
  splits the documentation and the tests in two for a difference of one
  declaration.
* **Checking the whole input up front in a byte grammar**, then parsing as
  text. That is what a text grammar already is.
* **`from_utf8_unchecked` on anything but a proved-ASCII match.** A byte
  grammar must not be the way to get an unchecked `&str` out of bytes nobody
  controls.
* **Leaving it until a program needs it.** Byte input is basic equipment for a
  parser generator; what this repository's users call today is not what decides
  that.

## Tests

* a byte grammar over a binary format (a length-prefixed record) that a text
  grammar cannot take;
* the 1BRC shape as a byte grammar, `text(until(";"))` plus `dec<i32>`, with the
  same result as the text grammar on the same input;
* `text(p)`, `intern(p)`, `raw_ident` on a bad byte: a `Cut`, at the absolute
  offset of that byte, with the message of D5 - first byte, middle, last, and a
  truncated multi-byte character at the end of the input;
* `intern(p)`: a second occurrence of the same name is not checked again
  (a counting `Interner` whose `lookup_bytes` default is replaced);
* a user-declared interner without `lookup_bytes` still compiles and still
  rejects a bad byte;
* `par_fold` with `input bytes;`: pieces against the sequential parse;
* every existing test unchanged: a grammar without `input bytes;` generates
  what it generated before;
* UI tests: `input bytes;` twice; `bytes(p)` in a text grammar.
