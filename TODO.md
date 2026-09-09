# Remaining High-Priority Tasks

What is actually open. An item leaves this file when it is closed - by the
change and the test that would fail without it, or, where the answer was that
nothing should change, by the measurement or behaviour that says so, written
down where the next person will look rather than here. The history of what was
decided lives in `CHANGELOG.md`, the reasoning in `docs/adr/`, the numbers in
the benchmark headers, and the rules a grammar author needs in `SYNTAX.md`.

## 1. A frame that ends in a rule which *is* its boundary

`#[frame(boundary = "\n")] LINE -> &'a str = s:until(NL) NL` does not compile,
with `NL -> () = "\n"`. The trailing element is accepted - `is_terminator`
resolves a rule to its literals - but `NL` is then also walked as an interior
rule of its own, where its literal is the boundary and is flagged.

Making it compile needs **position-sensitive reachability**: knowing that `NL`
is reached only as the terminator, which is the one position the interior check
already excludes. Today the walk follows calls without caring where the call
sat.

Pinned as a rejection in `tests/ui/frames.rs`, so the message says what it is
rather than the grammar merely failing. Small enough to be worth doing when
someone hits it; not worth doing on speculation, since repeating the literal is
a one-character workaround.

## 2. Merging across the pieces of a `par_fold`

A merge is `Fn(T, T) -> T`: it sees two values, and `rt::parse_piece` has
already dropped the piece's context by then. So identity cannot cross a piece
boundary unless the pieces shared what produced it. Symbols do share, since
`parse_<rule>_pieces` clones one context into every piece; a table in
`user_state` does not, unless the caller shares a handle and pays a lock.

The fastest shape - a table per piece, merged by name - needs a per-piece
`finish` that sees its own context before it is dropped.
`docs/adr/adr21-merging-across-pieces.md` has the four options and what each
costs. **Proposed, not scheduled**: what decides it is a measurement of the
shared table's lock against the per-piece table on more cores than the four
this was written on.

`docs/adr/adr22-the-interner-is-the-choice.md` says why this is one design and
not two: whether a merge must remap is decided by *which interner* the context
holds, along with whether it is shared and whether it must be thread-safe.
Build them together, after that measurement.

## 3. The byte driver

`ParseInput<'a, S>` is `LocatingSlice<&'a str>`, so every generated parser
takes `&str` and every built-in is written against it. A format that is not
UTF-8 - a binary record, a length-prefixed field, a file whose encoding is the
grammar's business rather than the reader's - has no way in.

What already points that way: `rt::frames_bytes` cuts `&[u8]` and is what
`rt::frames` is a `&str` view of (ADR 16 §4), so the piece machinery does not
assume text. The character classes now match on bytes as well (ADR 23):
`AsciiClass::run` takes a `&[u8]` and would carry over unchanged - only
`raw_ident`'s decoded tail is about text at all. The error engine takes `I: Stream + Location + AsBStr`, which
`&[u8]` satisfies. What does not: the `ParseInput` alias, the built-ins
(`ident`, `digit1`, `string`, the character classes), `text(p)` and `intern(p)`
yielding `&'a str`, and the whitespace rules.

The shape of the question is whether the input type becomes a parameter of the
generated code - which would put it in every signature beside `S` and `E` - or
whether a byte grammar is a second, narrower set of built-ins over a fixed
`&[u8]` input. That is an ADR, and it should be written before anything is
built: it decides how much of the language a byte grammar shares with a text
one.

## 4. `count(p)` pays for keeping its count

Three loops over 200_000 digits, element for element identical and differing
only in what they do with each one: taking the run as text **173 µs**,
collecting the elements of a rule **262 µs**, `count(p)` **483 µs**
(`benches/repetition.rs`). The counter being live is the whole gap, ~1.6 ns per
element - a repetition that discards its count used to pay the same and stopped
when codegen started taking the text instead of mapping the count away.

The obvious answer for a character class is to count from the slice it matched:
`take`, then count the characters in it, which is one vectorisable pass instead
of a live counter. **Unmeasured**, and worth measuring before building - the
second pass is not free either, and `count(p)` over anything but a character
class cannot use it.

## 5. The word-at-a-time class scan below its break-even

ADR 23's changelog entry says of the eight-bytes-at-a-time scan that on short
runs it *"is smaller but never negative."* On a bounded run of one or two
characters it is negative.

`AsciiClass::out_mask` costs ~13 operations per 8-byte word for a one-range
class, and with `run`'s loop head, `try_into`, `trailing_zeros`, the divide and
`class`'s `as_bstr`/`next_slice` a call is **~30 instructions regardless of run
length**, against ~4-5 *per character* for the predicate loop. Break-even is
therefore around six to eight characters, and a one-character run cannot come
out ahead.

Measured end to end by Nikaia on 1BRC (`digit{1,2}` and `digit`), callgrind
over 200 000 rows, same source and flags, identical output - simulator numbers,
so they do not depend on the machine:

| | `2f0d5da` | `024e3d3` | Δ |
| :--- | ---: | ---: | ---: |
| instructions | 131.2 M | 143.9 M | +12.7 M |
| branches | 16.49 M | 17.25 M | +0.76 M |
| **mispredicts** | 653 026 | 638 959 | **−14 067** |
| D1 misses | 207 908 | 209 753 | +1 845 |

The scan does what a word scan is supposed to do - it buys fewer mispredicts
with more instructions - and the size of the trade is what is wrong: 14 067
mispredicts at ~20 cycles is ~0.28 M cycles won against ~3.2 M paid at a
generous IPC of 4. About elevenfold against. The scan path itself goes 40 ->
106 instructions per row, which is the whole difference.

This is a threshold, not a regression to undo: ADR 23's +9% on
identifier-heavy grammars is entirely plausible, identifiers being 5-15
characters. **Next step:** measure where the crossover actually sits on a real
machine, then have the code generator take the character path where it knows
the upper bound is below it - `{1,2}` is a compile-time fact.

## 6. Release readiness for 0.1.0

`CHANGELOG.md` has collected real breaking changes under *Unreleased* -
`parse_<rule>_pieces` taking a context, `Diagnostics` gaining a method,
`ParseContext` gaining fields. Before a release: check that every one carries a
migration note, that the examples in `README.md` and `SYNTAX.md` still compile
as written, and that nothing in them describes a version that no longer exists.
Twice in one week a stale sentence sent someone down the wrong path - an
attribute that never existed, and a return type that had changed - so this is a
pass over the documents, not over the code. **Not before §1-§5**: there is
still language missing.
