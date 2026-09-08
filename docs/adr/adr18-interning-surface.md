# ADR 18: Interning — One Builtin, Two Escape Hatches, No Operator

**Status:** Proposed. **Date:** 2026-09-08.
**Tests:** none yet. The three paths below were verified by hand against the
current tree; `tests/interning_test.rs` covers only the first of them.

## Context

ADR 14 settled *where* the interner lives: a `ThreadedRodeo` behind an `Arc`
in `InternerContext`, reachable from every generated parser as
`input.state.interner`, owned by the caller and shared across parses. It did
not settle *what a grammar may intern*. That question was never asked,
because the only thing that ever interned was `ident`, and the answer was
therefore "identifiers".

Three paths reach the interner today. Only the first is intentional.

**1. The `ident` builtin.** `codegen/expr.rs` turns `ident` into
`take_while(alphanumeric | '_')` followed by
`input.state.interner.intern_string(s)`, and the backend declares its return
type as `Symbol`. Its twin `raw_ident` parses the same characters and returns
`&'a str`. This pair is the whole designed surface: identifiers are interned,
everything else is not, and no rule says why.

**2. An action block, through `_state`.** `codegen/variants.rs` scans the
action's token text for `_state` and, on a hit, injects a binding for the
context ahead of the action. A grammar can therefore write

```rust
pub city -> Symbol = s:until(";") -> { _state.interner.intern_string(s) }
```

The injection is `let _state = ::winnow::stream::Stateful::state_mut(#input);`
and **winnow 0.7 has no such method** — `Stateful` exposes `state` as a public
field. Every grammar that names `_state` in an action fails to compile with
`no function or associated item named 'state_mut'`. No test names it and no
document mentions it, which is why the breakage has gone unnoticed. Replacing
the injection with `let _state = &mut #input.state;` makes the rule above
compile and intern; that was confirmed against the current tree.

**3. An `extern rule`, through the hand-written-parser fallthrough.** A name
that is neither a user rule nor a builtin is emitted as a bare path and driven
as a winnow parser over `&mut ParseInput`, so a hand-written parser reaches
`i.state.interner` like any generated one:

```rust
extern rule city -> Symbol;

fn city<'a, S: Clone + Debug>(i: &mut ParseInput<'a, S>) -> Result<Symbol, ParseError> {
    let s: &str = take_till(1.., ';').parse_next(i)?;
    Ok(i.state.interner.intern_string(s))
}
```

This works today, unchanged, and is also undocumented.

So interning arbitrary text is *possible* but only by leaving the grammar
language — into a Rust action that currently does not compile, or into a Rust
function. What is missing is not a capability. It is a way to say it in the
DSL.

## Decision

### 1. `intern(pattern)` becomes a builtin that takes an argument

`intern(p)` runs `p`, requires its output to be `AsRef<str>`, and yields the
`Symbol` for that text:

```rust
pub city  -> Symbol = intern(until(";"))
pub key   -> Symbol = intern(raw_ident)
pub quoted-> Symbol = intern(string)
```

It is a **builtin with one pattern argument**, not a new `ModelPattern`
variant. That choice is about cost, and the cost is lopsided: a new variant
needs an arm in the pattern parser, the model, the validator, the frame
analysis and roughly a dozen `match`es in `analysis.rs` and `codegen/expr.rs`;
a builtin needs one arm in the builtin `match` of `generate_rule_call_parser`
— which already has `args` in scope — plus one `BuiltIn` entry in
`WinnowBackend::get_builtins`. Validation already passes, because builtin
names go into the validator's set of definitions and argument patterns are
validated recursively. Nothing else changes: to every other pass `intern(p)`
is an ordinary `RuleCall` with an argument, which the frame and nullability
analyses already handle.

The generated code is the shape `ident` already has:

```rust
(|i: &mut ParseInput<'a, S>| -> Result<Symbol, ErrMode<E>> {
    let s = #inner.parse_next(i)?;
    Ok(i.state.interner.intern_string(s.as_ref()))
})
```

which makes the second half of this decision free: **`ident` is
`intern(raw_ident)`**. The builtin keeps its name and its return type, but its
codegen arm collapses into the new one instead of repeating it. That is the
argument that the operator is the right shape — it is not a new mechanism, it
is the name of one that exists in a single hard-coded place.

Consequences of being a map over the inner parser, all of them wanted:
whitespace stays the inner pattern's business, so `intern(p)` consumes leading
whitespace in a syntactic rule exactly where `p` would; a failing `p` fails the
`intern`, with `p`'s own error and position; and in a `lex(...)` or `#[lexical]`
rule it is strict, like everything else there.

A non-`AsRef<str>` argument — `intern(u32)` — is a Rust type error at the
generated call site, not a grammar error. Accepted: the same is true of every
type mismatch between a pattern and an action today.

### 2. `_state` is the escape hatch, and it must compile

Everything `intern` deliberately does not do — normalizing before interning
(`s.to_lowercase()`), interning into a second interner, interning a value the
grammar computed rather than one it parsed — belongs in an action block, which
is where a grammar puts Rust anyway. That path stays, and this ADR fixes it:
the injection becomes `let _state = &mut #input.state;`, gains a regression
test, and is documented as what it is — the general way an action reaches
`ParseContext`, of which the interner is one field.

Two known weaknesses of the mechanism are recorded rather than fixed. The
trigger is a substring search over the action's token text, so `_state` inside
a string literal or a comment injects a binding nobody asked for (harmless: it
is an unused `let` on an already-finished borrow), and any renaming of the
binding is a breaking change to grammars that use it. A cleaner trigger would
be to inject unconditionally and let the `_`-prefix silence the warning; that
is a separate change, and it should be measured against compile time before it
is made.

### 3. What is not decided here

**No return-type coercion.** A rule declared `-> Symbol` whose body yields
`&str` could intern implicitly. Rejected: it would make the interner reachable
without any token in the grammar naming it, and the difference between an
interned and a borrowed field is exactly the thing a reader of the grammar
should see.

**No second interner in the DSL.** `intern` always means
`state.interner`. A grammar that needs two goes through `_state`.

**No performance work.** The obvious next question — that
`ThreadedRodeo::get_or_intern` hashes with SipHash and locks a dashmap shard
per identifier, and that a small per-`ParseContext` cache in front of it would
be lock-free because `rt::fold_pieces` builds one context per piece — is a
separate ADR, and it is not writable yet: the repository has no `benches/`, so
every claim about it would be a guess. `intern` makes that work more valuable
(interning becomes reachable for the repetitive fields of a data format, not
only for identifiers) without depending on it in either direction.

## Consequences

* **The interner is monotone under backtracking.** An alternative that
  interns and then loses leaves its symbols behind. This is already true of
  `ident` inside an `alt`; `intern` makes it easier to hit, on longer text.
  No symbol is ever wrong — the interner just holds more than the parse
  result names. Accepted; the alternative (interning after commit) would mean
  buffering, which costs more than the entries do.
* **ADR 17's replay is unaffected.** That ADR already promises interning is
  idempotent: the fast pass and the diagnosing replay intern the same text and
  get the same symbol. `intern` inherits the promise unchanged.
* **Symbols are meaningful only against the interner that made them.** IDs
  are assignment-ordered, so they depend on parse order and, across the pieces
  of a `par_fold`, on thread interleaving. Two rules follow, and this ADR
  states them because `intern` puts symbols in front of many more users than
  `ident` did: never compare or resolve a symbol against a different
  `InternerContext`, and never rely on the numeric order of two symbols. The
  trap has teeth in `rt::fold_pieces`, whose `new_context` closure builds a
  *fresh* `ParseContext` per piece — a closure that calls
  `InternerContext::new()` instead of cloning a shared one gives every piece
  its own numbering, and the merge then compares symbols that mean nothing to
  each other. ADR 16 describes cloning the `Arc`; nothing enforces it. Worth a
  documented example next to `Parallelism`, and possibly a debug assertion.
* **Documentation debt this uncovered, to be paid with the implementation.**
  `README.md` lists `ident` as returning `String` — it returns `Symbol`.
  `SYNTAX.md`'s builtin table describes `ident` as "an identifier" without
  saying it interns, and never mentions that `raw_ident` is the
  non-interning twin. Neither `_state` nor `extern rule` appears in either
  document, although both are implemented and one of them works.
