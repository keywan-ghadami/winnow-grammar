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

## 3. Release readiness for 0.1.0

`CHANGELOG.md` has collected real breaking changes under *Unreleased* -
`parse_<rule>_pieces` taking a context, `Diagnostics` gaining a method,
`ParseContext` gaining fields. Before a release: check that every one carries a
migration note, that the examples in `README.md` and `SYNTAX.md` still compile
as written, and that nothing in them describes a version that no longer exists.
Twice in one week a stale sentence sent someone down the wrong path - an
attribute that never existed, and a return type that had changed - so this is a
pass over the documents, not over the code.

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
