# Changelog


## [0.1.0] - Unreleased

### Breaking Changes

- **A repetition of a character class yields the text it matched**, not its
  elements: `digit{1,2}`, `digit*`, `digit+` and `any{n}` are now `&'a str`
  instead of `Vec<char>`. `{n,m}` is borrowed from regular expressions, where
  `\d{1,2}` matches text and not a list of characters, and the `Vec` was a copy
  of input the parser had already walked over - no grammar in this repository
  wanted it, all of them turned it straight back into a string, a count or a
  number. `digit*` is now the `digit0` the built-in table never had, and
  `digit+` the `digit1` it has.

  The classes are `digit` and `any` - they consume one character and yield it
  unchanged. `char` is **not** one although it yields a `char`: it parses a
  character *literal*, so `'\n'` is four characters of input and one of value,
  and its text is not its value. A repetition of a rule still yields its
  elements, because those are values the parser built.

  **Migration**: `d.iter().collect::<String>()` becomes `d.to_string()`,
  `for c in d` becomes `for b in d.bytes()`, `d[0]` becomes an index into the
  slice or a `d.chars()`. `d.len()` keeps working and still counts the digits.
  In a syntactic (lowercase) rule the text now includes the whitespace between
  the elements, since that is what was consumed - a numeric run belongs in a
  lexical rule, as it already did.

  It is also the last allocation on the 1BRC path: `TENTHS` measures 60 ns ->
  29 ns (`benches/repetition.rs`, one machine), which is below the same rule
  written by hand.

- **`parse_<rule>_pieces` takes a context, not a factory** (ADR 19 §2). Every
  piece parses with a clone of it, so the interner inside is shared and symbols
  from two pieces mean the same thing. That was possible before and nothing did
  it: the factory made the safe answer the one you had to know to write, and
  every call site in the tests and the documentation passed
  `ParseContext::default`, giving each piece an interner of its own — two
  different words could share an id, one word could have two, and nothing
  failed.
  - **Migration**: `parse_X_pieces(input, ParseContext::<()>::default, how)`
    becomes `parse_X_pieces(input, &ParseContext::<()>::default(), how)`.
  - A `user_state` that must start empty in every piece — a slot table whose
    numbers are the piece's own, an accumulator that must not be copied — uses
    the new **`parse_<rule>_pieces_with`**, which keeps the closure. For that
    workload it is the ordinary entry point; the surprising answer is now the
    one you have to name.
  - Unchanged: `rt::fold_pieces` still takes a factory, being the layer a
    caller with its own executor uses.

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

- **Lazy diagnostics** (`docs/adr/adr17-lazy-diagnostics.md`). Every entry
  point first parses with a zero-sized error type and only diagnoses a
  failure, by parsing again with the engine above; on a `par_fold` rule from
  the item the fast pass stopped in. Messages are unchanged. What changes:
  - `ParseContext` has two more fields, `diagnose: Diagnose` (`Replay`,
    `ReplayInPlace`, `Off`, `Eager`; default `Replay`) and `fold`;
    `ErrorCore` has `undiagnosed`. `Default` users are unaffected.
  - An action that mutates `user_state` is replayed after a failure - on a
    clone-restored state under `Replay`. See the ADR's side-effect contract.
  - `rt::fold_pieces` takes the rule's two instantiations (`EmptyError`,
    `ParseError`) instead of one parser; `rt::RtError` is now the bound the
    generated code puts on its error type parameter, and the runtime helpers
    are generic over it. Rules cannot name a generic parameter `E`.
  - **Migration**: none for a grammar; a caller that wants the old behaviour
    sets `ctx.diagnose = Diagnose::Eager`.

### Fixed

- **`text(p)` and `dec(p)` collected what they then threw away.** Their inner
  pattern was generated as if its values were wanted, so `text(digit{1,2})`
  built a `Vec<char>` and `.take()` dropped it - the operators cost exactly
  what they were added to remove, which the benchmark said plainly once it
  measured the generated code instead of a hand-written stand-in (`TENTHS`
  60 ns before, 61 ns with `text`). Their elements are now generated as
  discarded, which is what the grammar already says by naming none of them:
  60 ns -> 30 ns.

- **A repetition whose elements nobody names no longer collects them.** `x*`
  and `x+` without a binding, `x{n,m}` without one, and `count(x)` built a
  `Vec` and then threw it away or asked only for its length. The grammar had
  already said the elements were not wanted - it named none - so
  `rt::repeat_counting` counts instead. Memory goes from proportional to the
  input to constant: over 200 000 elements the peak drops from ~5.2 MB to
  nothing, which `tests/repetition_memory_test.rs` pins with a counting
  allocator. Time, measured with `benches/repetition.rs` on one machine: a
  short repetition roughly halves (`digit{1,2}` discarded 49 ns -> 23 ns,
  `digit*` over five 93 ns -> 34 ns); over 200 000 elements the collecting and
  counting forms are within a few percent of each other. Bounded repetitions
  that *do* bind their elements gained about 12% on the way (`digit{1,2}`
  50 ns -> 44 ns), because the upper bound is now a plain number rather than an
  `Option` unwrapped once per element.

- **An `extern rule` behind an attribute is parseable.** The grammar body chose
  between a rule and an `extern rule` by peeking at the first token, but both
  parse attributes first, so `/// doc` in front of a declaration routed it to
  the rule parser and failed with `expected \`=\``. `ExternRule.attrs` had been
  unreachable since it was written. The choice now looks past the attributes,
  which also makes the attribute check reach an `extern rule`.

- **A rule that only lookahead reaches is no longer checked against a frame
  boundary.** `peek(…)` and `not(…)` consume nothing, so nothing they contain
  can carry the parser past the boundary - but the reachability walk followed
  them anyway and rejected sound grammars (a rule whose body is `multispace0`,
  used only as `peek(BLANKS)` under a `"\n"` frame). The walk that resolves
  `frame_end` still follows lookahead, because `peek(frame_end)` names the
  boundary as much as consuming it does. `check_pattern` had always treated
  lookahead *written inside* a rule this way; the walk now agrees for a rule
  reached *through* it.

- **A context used for a second parse numbered its items from the first one's
  total.** `fold.base` numbers a `par_fold`'s items and is advanced when a
  parse fails; nothing reset it, so the next parse through the same context
  counted from where the last one stopped - `in item 7` for what a fresh
  context called `in item 4`, growing by the item count with every failure.
  That is the case ADR 14 is built for: one long-lived context, several source
  files. `rt::entry` and `rt::entry_framed` now begin by clearing the
  diagnostics engine's working space (`ParseContext::begin_parse`: `fold`,
  `furthest`, `rules`), which is where a parse takes ownership of it.

  The defect was confined to the message - what a parse accepted and returned
  never depended on it - and to `par_fold` rules, since a plain `fold` runs
  untracked. `tests/context_reuse_test.rs` pins both the fix and those
  boundaries, and the tests covering the defect were checked to fail without
  it. See `docs/adr/adr19-one-context-per-parse-not-a-factory.md` §1.

- **`_state` in an action block did not compile, and was detected by a text
  search.** The code generator injects a
  binding for the `ParseContext` when an action names `_state`, and injected
  `winnow::stream::Stateful::state_mut(input)` - a method winnow 0.7 does not
  have (`Stateful` keeps its state in a public field). Every grammar that named
  `_state` in an action failed with `no function or associated item named
  'state_mut'`; no test named it and no document mentioned it, so the breakage
  went unnoticed. The injection is now `&mut input.state`, and
  `tests/state_action_test.rs` covers all three injection sites - a plain
  variant, one with a span binding, and the left-recursive loop body. Note that
  `_state.user_state` is still not usable from an action: a generated rule is
  generic over the state type, so only the context's own fields (the interner
  among them) have a known type there; a grammar that has to touch its user
  state does it in an `extern rule`. The binding is now injected into *every*
  action rather than into those whose token text contains `_state`: that search
  fired on the word inside a string literal or a comment, and made the
  binding's name a hidden part of the API. An action may bind the name itself,
  which shadows the injected one. See `docs/adr/adr18-interning-surface.md` §2.

- **`README.md` documented the wrong return types for two built-ins.** `ident`
  returns `Symbol`, not `String`, and `string` borrows `&'a str` rather than
  allocating a `String`.

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

- **`dec<T>(p)`: the text `p` matched, read as a number.** `dec<i32>(digit{1,2})`
  where the action used to fold characters by hand. Not a faster way to parse a
  number - measured, it costs what `text(p)` plus that fold costs
  (`benches/repetition.rs`) - but the fold is no longer written out at every
  numeric field, and a value the named type cannot hold becomes a parse error
  that says "number too large to fit in target type" instead of wrapping in
  release and panicking in debug. That reason is why it is not winnow's
  `parse_to`, which discards the `FromStr` error and reports a bare position.
  The type is the one the call names, through the generics a call already
  takes; without one it is inferred. It costs about 5 ns more than `text(p)`
  and the same fold in the action, `str::parse` validating more than two bytes
  need - which is the price of the two properties above, not a saving.

- **`text(p)`: what was matched, not what was parsed.** Runs `p` and hands back
  the input it consumed as `&'a str`, borrowed from the input - no allocation,
  whatever `p` is. Several patterns are a sequence, not several arguments, so
  `text(digit{2} raw_ident)` captures both and the whitespace between them is
  whatever it would be outside the `text`.

  It settles an inconsistency the language already had: `digit1` yields
  `&'a str` and `digit{1,2}` a `Vec<char>`, though both are a run of digits.
  That is why `intern(until(";"))` worked and `intern(digit{3})` could not - a
  `Vec<char>` is no `AsRef<str>`. `intern(text(digit{3}))` now does.

  It is also where the 1BRC temperature's time was: `TENTHS` written with
  `text(digit{1,2})` instead of a `Vec<char>` measures 60 ns -> 30 ns over
  three runs (`benches/repetition.rs`, one machine) - half the rule, and below
  the same thing written by hand. The whole difference is not copying two
  characters onto the heap.

- **An attribute a rule carries must be one the generator reads.** `#[frame(…)]`
  and doc comments are it; anything else is now a spanned error instead of
  being dropped in silence, so a typo (`#[frmae(boundary = "\n")]`) no longer
  leaves a rule quietly not a frame. Doc comments are forwarded to the
  generated parser rather than discarded. The check found three of these in
  this repository's own tests and one in `SYNTAX.md`: `#[lexical]`, which
  never did anything - a rule is lexical because its name starts with a
  capital - and two `#[allow(dead_code)]` on `WS`, which the generator already
  emits itself. **Migration**: delete the attribute; none of them had an
  effect. `#[allow(…)]` is not forwarded, deliberately - the generated module
  already allows the lints a grammar runs into, and each forwarded attribute
  would be a promise about which of the generated items it lands on.

- **`winnow`'s `simd` feature is enabled**, so the scans behind `until(…)` and
  `recover(…)` go through `memchr` as this crate's own documentation already
  said they did. Without it, `find_slice` compiles to the portable fallback -
  a byte-at-a-time `position` for a single byte and a naive substring search
  for a literal - and no amount of prose about SIMD made that true. Measured
  over 8 MiB of `until(";" | frame_end)` records in release on one machine:
  **11.0 ms -> 6.7 ms**. Nothing about behaviour changes; `memchr` joins the
  dependency tree through `winnow`.

- **`intern(pattern)`: a `Symbol` from any text a grammar can parse.** ADR 18.
  Interning used to be reachable only through `ident`, so a grammar that wanted
  a symbol for a field value, a quoted string or a rule of its own had to leave
  the DSL. `intern(p)` runs `p` and interns what it yields (anything
  `AsRef<str>`): `intern(until(";"))`, `intern(string)`, `intern(my_rule)`.
  **`ident` is now exactly `intern(raw_ident)`** - the same combinator
  (`rt::intern`), not a second copy of it. `intern` normalises nothing; an
  action block reaching the context as `_state` is where normalising belongs.
  `intern` with any other number of arguments is a compile error pointing at
  the word `intern`. It is also the first built-in besides `separated` and
  `repeated` to take a positional argument, which is a hard-coded list in the
  grammar parser because parsing runs before the backend is known; an arity on
  `BuiltIn` is the general fix (`feature-requests.md` §1).

- **`winnow_grammar::span`: byte offsets, and the place they name.** A `@`
  binding yields a `Range<usize>` and keeps doing so - offsets are what
  `LocatingSlice` knows, they cost nothing and they slice the source, while
  line and column cannot be computed without the source at all. That step now
  has a home: `line_column(source, offset)`, and a `SpanExt` for
  `Range<usize>` with `text(source)` and `line_columns(source)`.
  `ErrorCore::line_column` calls the same function, so a span and a message
  cannot disagree about where something is. Spans are also documented in
  SYNTAX.md for the first time, `@` and `@=` both.

- **`recover(…)` keeps what it swallowed.** It was
  `alt((body.map(Some), (skip, sync).map(|_| None)))`, which discarded the
  failure where it was produced, so a parse could recover from ten errors and
  report none of them. `ParseContext::recoveries` now counts them - in both
  passes, so it is there after the successful parse that a recovering one is -
  and `ParseContext::recovered` holds the errors themselves wherever the
  diagnosing engine ran. Under the default `Diagnose::Replay` a recovering
  parse succeeds on the fast pass alone, which has no error object to keep:
  the count says that something was recovered, `Diagnose::Eager` says what.
  A cut inside the recovered rule is still fatal, as it was.
  - Replaying automatically when the count is non-zero is ten lines in
    `rt::entry` and deliberately not taken: it would make a successful parse's
    cost depend on its input, which ADR 17 promises it does not. The count is
    what would make it possible if the two-step proves clumsy.

- **A lookup cache in front of the interner.** `ParseContext` carries a
  direct-mapped table of 512 slots (8 KiB), and `ParseContext::intern` - what
  `ident` and `intern(…)` call - answers from it before asking the interner.
  A hit costs **9.4 ns against 22.9**, and a parse of identifier-heavy input
  runs **43% faster** end to end (`benches/interning.rs`). A miss falls
  through; the interner remains the only authority on what a `Symbol` is, so
  the cache cannot make one wrong - `tests/interning_test.rs` checks that,
  including words that share their first eight bytes, slots displaced 4000
  times over, a context whose interner is replaced, and a clone.
  - `_state.intern(…)` in an action is the cached path;
    `_state.interner.intern_string(…)` still goes straight to the interner and
    returns the same symbol.
  - `ParseContext` gains a `#[doc(hidden)]` field for it. Code that builds the
    struct with `..Default::default()` is unaffected.

- **`state T;`: a grammar declares the state it needs** (ADR 20). Until now
  nothing a grammar could express touched `user_state`: rules are generic over
  the state type, so `_state.user_state` had type `S` in an action, and a
  hand-written parser - called from that same generic code - could not name a
  concrete state either. A grammar now declares one, and an action reaches it
  as `_state.user()`.

  The declaration is a **bound, not a substitution**: rules stay generic and
  require only that the state provides a `T`, so a state that *is* a `T`
  satisfies it with nothing to write, and one composite state serves several
  grammars at once by implementing `StateOf<T>` for each. Nothing existing
  changes - a grammar that declares no state generates what it generated
  before, `parse_<rule>_pieces` and its `new_context` closure included.

  With it: `winnow_grammar::StateOf`, `ParseContext::with_state` and
  `with_state_and_interner` (`Default` is not required of a state), and
  `testing::WinnowTestExtWith::parse_test_in(state, input)` beside the
  `()`-fixed `parse_test`. A state that provides nothing of the sort is
  rejected in the grammar's own words rather than as an unresolved type
  parameter. This is what makes the 1BRC shape expressible: a hand-written
  parser assigns slots out of the declared state and the fold aggregates by
  them - `tests/state_test.rs`.

- **`Symbol::index()`, and `InternerContext::len()`/`is_empty()`.** A symbol's
  position in its interner is dense, zero-based and assigned in first-seen
  order, so the `n` distinct strings an interner holds have the indices `0..n`.
  That makes a symbol the row number for a caller's own `Vec` of accumulated
  data - the aggregation shape, with no second lookup - and `len()` is the size
  that `Vec` needs. The number was there all along (`Symbol` stores
  `index + 1`); only `#[doc(hidden)]` conversions could reach it. The contract
  is documented with it: the index means nothing outside the interner that
  produced it, it depends on the order strings were first seen, and it is not
  to be persisted.

- **Benchmarks.** `benches/interning.rs` measures interning where it happens -
  the interner alone, an identifier-heavy grammar, and the 1BRC shape with
  `intern`, sequential and cut into pieces. `benches/where.rs` takes a single
  `intern_string` apart: of ~21 ns, ~14 ns is hashing and probing, ~6 ns the
  dashmap shard lock. Its header reasons from those numbers - including three
  hasher replacements that measured *slower* than the default, and the lookup
  cache that took all of it instead.

- **`extern rule` is documented.** The declaration existed and worked - a
  hand-written parser reaches `i.state.interner` like any generated one - but
  appeared in no document. SYNTAX.md gains the section, including the signature
  it needs (`fn(&mut ParseInput<'a, S>) -> Result<O, ParseError>`, not
  `ErrMode<ParseError>`) and the note that it runs in both passes of ADR 17.
  `tests/extern_rule_test.rs` pins the documented shape.

- **The shared interner of ADR 14 has a worked example and a test.**
  `parse_<rule>_pieces` calls its `new_context` closure once per piece, so
  `ParseContext::default` - what every call site passed, including the
  documentation - gives each piece an interner of its own, and symbols from two
  pieces are not comparable: two different words can share an id, one word can
  have two, and nothing fails. `tests/shared_interner_test.rs` shows the closure
  that clones one interner into every piece, and pins the fresh-per-piece
  behaviour beside it; `SYNTAX.md` gains the condition next to its
  `parse_FILE_pieces` example. ADR 18 §3.

- **`#[frame]` and `par_fold(rule, init, step, merge)`: parsing in pieces.**
  ADR 16. A rule marked `#[frame]` claims it can be found from any offset by
  scanning to the next boundary (the literal it ends in, or
  `#[frame(boundary = "\n")]`). The claim is **checked, never repaired**: every
  rule reachable from the frame is walked, and each thing that consumes input
  is *safe* (a literal without the boundary, a built-in whose alphabet cannot
  include it, lookahead, an `until(…)` whose terminator covers the boundary)
  or *rejected* with the rule and pattern named (a literal containing the
  boundary, `any`, `multispace0`, the implicit whitespace of a syntactic rule,
  an `until(…)` that does not cover the boundary — the message says
  `until(… | frame_end)` — and `recover(…)`). Nothing changes what a pattern
  means: the parser of a rule is the same whether or not a frame reaches it.
  **`frame_end`** is a new built-in naming the boundary of the enclosing
  frame — written once in the attribute, referenced in the rules, resolved
  statically (an error outside every frame, or under two boundaries).
  `#[frame(…, unchecked)]` skips the walk, greppably. A frame must end in its
  boundary. `par_fold` is `fold` plus a merge over a frame rule, must be the
  whole body of its rule, and its entry point — alone among the rules — skips
  no whitespace, so that a piece parses exactly as the same bytes do in the
  sequential parse. Generated next to the parsers: `frames_<RULE>(input, n)`,
  `merge_<RULE>(a, b)`, and `parse_<RULE>_pieces(input, new_context, how)`
  with `how: rt::Parallelism` (`Off`, `Pieces(n)`, `Auto`), which cuts,
  parses, merges and reports a piece's error at its offset in the whole
  input; with the new optional **`rayon` feature** the pieces run on rayon's
  global pool, without it in sequence with the same result. The split is byte
  based (`rt::frames_bytes`) and `rt::frames` its `&str` view. The attribute is
  a keyed list so the formats the check cannot see through yet (quoted fields,
  start patterns, escapes, a scanner of one's own — ADR 16 §5) get keys rather
  than a second syntax. `tests/frames_test.rs`, `tests/ui/frames.rs`.
  Validation returns its analysis (`validator::Validated`, carried by
  `ParsedGrammar`) and the generator consumes it, so the frame check runs
  once.
- **`until(…)` takes an alternation, and a few fixed alternatives are scanned
  in one pass**: `until(";" | frame_end)`, `until("," | line_ending)` — up to
  three needles via `memchr2`/`memchr3` (`rt::scan_to_any`). More than three,
  or an alternative that is not a fixed string, take the position-by-position
  path.
- **`until` and `recover` scan for a fixed terminator** instead of running the
  terminator's parser once per character. A literal terminator, and the built-in
  `line_ending`, are found with `find_slice` — `memchr`, so SIMD where the target
  provides it and the same word-at-a-time trick in portable code where it does
  not, with no `unsafe` in this crate; `until(eof)` is the rest of the input
  with no search at all. Measured over 4 MiB in release, on this machine:
  `until(";")` **2.1 s → 2.7 ms**, `until(line_ending)` **3.7 s → 2.6 ms**, a
  `recover` skip **2.1 s → 2.0 ms**.
  - This is the skip in `recover` as well, which is the expensive half of error
    recovery and is reached exactly when a file has many errors (the
    open item about the byte-by-byte skip).
  - `line_ending` is two shapes rather than one literal, so the scan finds `\n`
    and then looks at the byte before it: `\r\n` is left whole, a bare `\r`
    stays ordinary text, and an earlier `\n` is not skipped past — which
    scanning for `"\r\n"` would do.
  - A terminator that is not a fixed string keeps the old position-by-position
    path and now yields the same value as the scanned ones. `until` and
    `recover` share one skip generator, so they cannot drift apart.
  - A rule of the grammar's own named `line_ending` or `eof` is that rule, not
    the built-in — the precedence rule calls already had — and takes the slow
    path.
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
