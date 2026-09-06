# Changelog


## [0.1.0] - Unreleased

### Breaking Changes

- **`until(…)` returns the text it skipped** (`&'a str`) instead of `()`. Binding
  it (`s:until(";")`) was possible before but gave the unit value, so there was
  nothing a grammar could do with it; scanning produces the slice anyway, and
  discarding it would have cost an extra combinator to hand back something
  useless. Unbound uses are unaffected.
  - **Migration**: a binding that relied on the unit type stops compiling; drop
    the binding, or use the slice.

- **Own diagnostics engine.** `parse_<rule>()` returns `winnow_grammar::ParseError`
  instead of `winnow::error::ContextError`. The message text changes: `invalid X`
  becomes ``expected one of: `&`, identifier; found unexpected token `)` `` plus
  the rule stack (`in ty`, `in arg`, `in item 1`). Selection by progress, then
  priority, then aggregation — the contract is `docs/adr/adr15-diagnostics.md`,
  one test per point in `tests/diagnostics.rs`.
  - **Migration**: code that checks message text adjusts it. Hand-written
    parsers plugged into a grammar return `ErrMode<ParseError>`; `ParseError`
    implements `ParserError`, `AddContext<StrContext>` and `FromExternalError`,
    so winnow combinators keep working unchanged.
- **`ParseContext`** has two new fields, `furthest` and `rules`. Code that
  builds it through `Default` is unaffected.

### Fixed

- **`Symbol`'s round-trip was off by one, so `resolve` returned the wrong text.**
  `Symbol::from_spur` stored `Spur::into_inner()` (the raw key, already
  `index + 1`) while `into_spur` passed that value to `Spur::try_from_usize`,
  which treats its argument as an *index* and adds one again. As a result
  `InternerContext::resolve` returned the **next** symbol's string, and panicked
  with `Key out of bounds` on the most recently interned one. Both directions now
  go through `Key`'s index representation. The existing interning tests only
  compared symbols to each other, so they passed either way; three tests that
  actually resolve have been added.

- **The braced delimiter pattern `{ … }` generated `]` as its closing token.**
  `Braced` was dispatched with the closing string of the bracketed form, so a
  grammar that matched literal braces parsed the opening brace and its content
  and then failed at the closing one (`unexpected token \`}\``). The bracketed
  and parenthesised forms share the same code path and were correct, which is
  why it survived: nothing tested braces. `tests/delimiter_test.rs`.

- **`count(pattern)` did not compile in any shape.** Bound to a name it was
  emitted as `let n: Vec<_> = …` around a parser that had already mapped to
  `usize`, and the lexical branch emitted
  `::winnow::combinator::::winnow_grammar::rt::…`, which is not valid Rust.
  The feature was documented in `SYNTAX.md` and had no test.
  `tests/count_test.rs`.

- **A repetition could return fewer items than its lower bound.** When the
  element matched without consuming input, the zero-progress guard that keeps
  the loop from spinning also ended it - below `min`. `("a"?){3}` reported
  success with zero items. An empty match now counts towards the minimum (as
  in a regex, where `(a?){3}` matches the empty string) and the guard only
  stops the loop once `min` is reached. `repeat_recording` is now the
  open-ended case of `repeat_recording_bounded` rather than a copy of it, so
  `*` and `+` get the same fix.

- Parser parameters of generic rules (`list<T>(item)`) are substituted; missing
  type parameters are inferred from the argument.
- A whitespace cycle (`WS -> comment -> WS` through a syntactic comment rule)
  is reported at macro time instead of overflowing the stack.
- Action blocks may contain statements (`-> { let x = …; x }`).

### Added

- **`until` and `recover` scan for a fixed terminator** instead of running the
  terminator's parser once per character. A literal terminator, and the built-in
  `line_ending`, are found with `find_slice` — `memchr`, so SIMD where the target
  provides it and the same word-at-a-time trick in portable code where it does
  not, with no `unsafe` in this crate. Measured over 4 MB in release: a
  single-character terminator went from **1.29 s to 3.2 ms**, a multi-character
  one from **1.36 s to 11.4 ms**.
  - This is the skip in `recover` as well, which is the expensive half of error
    recovery and is reached exactly when a file has many errors (the
    `TODO.md` item about the byte-by-byte skip).
  - `line_ending` is two shapes rather than one literal, so the scan finds `\n`
    and then looks at the byte before it: `\r\n` is left whole, a bare `\r`
    stays ordinary text, and an earlier `\n` is not skipped past — which
    scanning for `"\r\n"` would do.
  - A terminator that is not a fixed string keeps the old position-by-position
    path and now yields the same value as the scanned ones.
  - Behaviour otherwise unchanged: the terminator is not consumed, and its
    absence consumes to the end of the input rather than failing. Tests in
    `tests/scan_test.rs`, including the UTF-8 boundary case — the scan works in
    byte offsets.
- **Bounded repetition `p{n}` / `p{n,}` / `p{n,m}`.** `*` and `+` say *unbounded*,
  so a grammar had no way to state a width a format actually fixes — and a
  backend that specialised for a fixed width anyway would be inventing a
  constraint the grammar never made. Bounds are greedy and possessive like the
  existing repetitions (`rt::repeat_recording_bounded`, sharing their handling of
  progress, backtracking and error recording): at the upper bound the repetition
  simply stops and whatever follows sees the rest of the input; below the lower
  bound the element's own error is the failure. An element that can match the
  empty input still owes the lower bound — `("a"?){3}` matches the empty input
  three times. A malformed bound is rejected at the bound itself (`{3,1}`,
  `{0}`, `{1,2,3}` — see `tests/ui/bounds.rs`).
  **Disambiguation:** a brace group is still the braced-delimiter pattern; only
  one whose content starts with an integer is read as a bound. The group that
  loses its bare form — braces around an integer literal — gets the keyword
  form `brace(2)`, the same escape hatch `( … )` has in `paren( … )`: a
  delimiter carries a keyword form for as long as its bare form means something
  else. (In this backend nothing was lost either way: `literal(2)` does not
  compile against `&str` input, so a brace group holding a bare integer never
  built. The keyword form is what makes the rule hold for a backend where
  integer literals are matchable tokens.) Documented in `SYNTAX.md`; tests in
  `tests/bounded_repetition_test.rs` and `tests/delimiter_test.rs`.
- **`brace(pattern)`** — the keyword form of the braced delimiter, mirroring
  `paren(pattern)`.
- **`digit` built-in** — a single digit (`char`), next to `digit1`'s greedy run
  of them. Fixed-width numeric formats need the single-character terminal:
  `digit{1,2} "." digit` is the shape a bounded repetition is for, and `digit1`
  would have swallowed the whole run before the bound could count anything.
- **`fold(rule, init, step)`** — a repetition that threads an accumulator rather
  than collecting into a `Vec`. `repeat`/`*` must materialise every item, which
  makes the collection, not the parse, the memory cost on large inputs; a fold
  reduces as it goes and runs in constant space. It shares `repeat`'s handling of
  progress, backtracking and error recording (`rt::fold_recording`), and like
  `rule*` it matches zero occurrences, so empty input yields the initial
  accumulator. Documented in `SYNTAX.md`; tests in `tests/fold_test.rs`.
- **Variant labels take effect.** `# "…"` used to be parsed and dropped; now the
  name becomes the expectation when the alternative fails at its boundary.
- **Built-ins name their expectation** (`identifier`, `integer literal`, …).
- **List items carry their index** (`in item 2`).
- **`ParseError::render(source)`** for line and column when using `parse_next`.
- **Inline Grammars**: Support for defining grammars directly in Rust code using `grammar!`.
- **EBNF Syntax**: Sequences, alternatives (`|`), optionals (`?`), repetitions (`*`, `+`), and groups (`(...)`).
- **Winnow Backend**: Generates efficient `winnow` parsers (`ModalResult<T>`).
- **Whitespace Handling**: Automatic whitespace skipping using `multispace0`.
- **Left Recursion**: Automatic compilation of direct left-recursive rules into loops.
- **Rule Arguments**: Support for passing arguments to rules.
- **Span Tracking**: Support for capturing spans with `@` syntax (using `LocatingSlice`).
- **Built-in Parsers**: `ident`, `integer`, `uint`, `string`, `char`, `hex_digit0`, `hex_digit1`, `oct_digit0`, `oct_digit1`, `binary_digit0`, `binary_digit1`, `float`, `space0`, `space1`, `line_ending`.
- **External Rules**: Support for calling custom or external `winnow` parsers.
- **Cut Operator**: Support for the cut operator `=>` to control backtracking.
- **Diagnostics**: Compile-time detection of indirect left recursion and unreachable alternatives (via `syn-grammar` 0.7).
- **Group Bindings**: Support for bindings on groups (e.g. `x:(a | b)`).
