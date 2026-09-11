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

## 5. The word-at-a-time class scan below its break-even — closed

The estimate this item was opened with was wrong, and measuring it said
something better than the fix it proposed.

**What was estimated.** `out_mask` at ~13 operations per word, plus `run`'s loop
head, `try_into`, `trailing_zeros`, the divide and `class`'s `as_bstr` /
`next_slice`, was put at ~30 instructions per call regardless of run length,
against ~4-5 *per character* for the predicate loop — break-even around six to
eight characters. The proposed fix was a threshold in the code generator, taking
the character path where it knows the upper bound is below it.

**What it actually is.** Callgrind over a run of `n` digits, both paths in one
binary, per call and including the harness:

| run | word | by char |
| ---: | ---: | ---: |
| 0 | 43 | 27 |
| 1 | 43 | 34 |
| 2 | 43 | 41 |
| 3 | 43 | 48 |
| 4 | 43 | 55 |
| 8 | 57 | 83 |
| 16 | 71 | 139 |

The per-character loop costs ~7 instructions a character, not 4-5, and the word
path's fixed cost is 9 above it rather than 25. **The crossover is at three
characters**, so a threshold in the code generator would buy at most 9
instructions on a one-character run — and a class that always matches exactly
one character is written as `digit`, which is a `one_of` and never reaches here.
There is no threshold worth having.

**Where the cost actually was.** The same table's first row: a run of *nothing*
cost the same 43 as a run of eight. The implicit whitespace skip runs between
every pair of elements of every syntactic rule, and in a language written
without gratuitous blanks most of those find nothing — so a parse pays at least
one empty run per token. `run` now tests the first byte before entering the word
loop: 21 instructions instead of 43 where there is nothing, 6 more where there
is something.

End to end, same source, same flags, callgrind so the numbers do not depend on
the machine:

| | before | after | Δ |
| :--- | ---: | ---: | ---: |
| Nikaia's compiler, 2 000 small functions | 281.6 M | 233.7 M | **−17 %** |
| Nikaia's 1BRC, 200 000 rows | 123.6 M | 120.0 M | −2.9 % |

For scale on the second row: the same binary with the word scan replaced
entirely by the per-byte loop is 119.4 M, so the guard recovers seven eighths of
what the scan costs 1BRC while keeping everything it buys on long runs. The
first row is the one that matters, and it is the shape ADR 23's +9% on
identifier-heavy grammars was measured on.

---

## 7. Two things measured beside the whitespace hoist, and both lost — closed

Recorded so they are not re-proposed. Same workload as the hoist: Nikaia parsing
2000 functions, 300 KB, callgrind with `--branch-sim` and `--cache-sim`.

**`rt::expected` guarded on `E::RECORDING`** — the wrapper reads
`current_token_start()` before running its parser, and an expectation is worth
nothing on the pass that discards messages. Skipping it there looked free and
cost **+1.4 %** (1,025.5 M → 1,040.0 M): the position read is a field load, and
the early return changed inlining for the worse. The idea is sound in shape and
the wrapper is not where the money is.

**Respelling `WS` as `multispace0 (COMMENT multispace0)*`** — in the *grammar*
rather than here, so it is Nikaia's business, but the finding travels. It halves
the class-scan attempts and trades in the other direction from everything else:
**+0.7 % instructions, −7.7 % mispredicts** (2,509,073 → 2,315,826). The
arithmetic favours it by roughly 2.6×, and it was still refused: it changes what
a parse error says, and Nikaia's error corpus caught that. An uncertain
performance trade bought with a certain regression in messages is not a trade.

---

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
