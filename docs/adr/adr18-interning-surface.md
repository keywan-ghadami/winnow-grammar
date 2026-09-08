# ADR 18: Interning — One Builtin, Two Escape Hatches, No Operator

**Status:** Accepted, implemented. **Date:** 2026-09-08.
**Tests:** `tests/intern_test.rs` and `tests/ui/intern_arity.rs` (§1),
`tests/state_action_test.rs` (§2), `tests/shared_interner_test.rs` (§3).
`tests/interning_test.rs` keeps covering `ident` itself.

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

One thing the estimate missed, found in implementation: which builtins may
take **positional** arguments is a hard-coded pair in the grammar parser
(`separated`, `repeated`), because parsing runs before the backend is known -
`parse_grammar` is generic over `B`, `syn::parse2` is not. Everything else
followed by `(...)` parses as a *group*, so `intern(until(";"))` reached
codegen as a call with zero arguments next to a parenthesised pattern. The
implementation adds `intern` to that pair and names the general fix where it
belongs: an arity on `BuiltIn`, with disambiguation from the backend's own
list - `feature-requests.md` §1.

The parser it generates lives in `rt`, not in the macro:

```rust
pub fn intern<'a, S, O: AsRef<str>, P, E>(mut p: P)
    -> impl FnMut(&mut ParseInput<'a, S>) -> Result<Symbol, ErrMode<E>>
where P: Parser<ParseInput<'a, S>, O, ErrMode<E>>
{
    move |input| {
        let text = p.parse_next(input)?;
        Ok(input.state.interner.intern_string(text.as_ref()))
    }
}
```

That is `rt::expected`'s shape, so the codegen arm is one line
(`rt::intern(#inner)`) and the behaviour is documented and testable where the
rest of the runtime is. `intern` with any other number of arguments is a
compile error pointing at the word `intern`.

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

What the hatch does *not* reach, found while testing it: `_state.user_state`.
A generated rule is generic over `S`, so the field is opaque there - the
`lazy_diagnostics` tests already say "a grammar action cannot do this to a
generic `S`" and write their state-mutating passes by hand. `_state` is
therefore the *context's own* fields: the interner above all, and `fold`,
`diagnose` and `rules` beside it. `tests/state_action_test.rs` covers all
three injection sites - a plain variant, one with a span binding, and the
left-recursive loop body.

Two known weaknesses of the mechanism are recorded rather than fixed. The
trigger is a substring search over the action's token text, so `_state` inside
a string literal or a comment injects a binding nobody asked for (harmless: it
is an unused `let` on an already-finished borrow), and any renaming of the
binding is a breaking change to grammars that use it. A cleaner trigger would
be to inject unconditionally and let the `_`-prefix silence the warning; that
is a separate change, and it should be measured against compile time before it
is made.

### 3. The shared interner gets a worked example and a test

ADR 14's central claim is that the interner is long-lived and shared. ADR 16
§3 says how a `par_fold` keeps that shape: `rt::fold_pieces` takes a
`new_context` closure, and the caller shares the interner by cloning it in
there. **Nothing in the repository ever did.** Every call site — the tests
(`tests/frames_test.rs:386`, `:444`) and the example in `SYNTAX.md:445` —
passes `ParseContext::<()>::default`, and `rt::parse_piece` calls that closure
once per piece (`src/rt.rs:260`), so each piece builds a *fresh* interner and
numbers its strings from one. The mechanism the ADRs are built on has never
been exercised.

It went unnoticed because no grammar in the suite crosses a piece boundary
with a symbol: `tests/frames_test.rs` is the 1BRC-shaped grammar, and its
`NAME` rule returns `&'a str`. Nothing in the suite binds `ident` under a
`par_fold`. A grammar that does gets no error, no warning and no panic — the
symbols are well-formed and resolvable *inside their own piece*, and wrong
everywhere else. Reproduced on the current tree, four rows cut into two
pieces:

```text
pieces = ["Hamburg;1\nZurich;2\n", "Zurich;3\nZurich;4\n"]
syms   = [Symbol(1),       Symbol(2),     Symbol(1),      Symbol(1)]
             Hamburg         Zurich         Zurich          Zurich
```

Piece 2 hands `Zurich` the id piece 1 gave `Hamburg`. Two different cities
now compare equal, and one city compares unequal to itself. Both are silent,
and a `merge` that counts by symbol produces a plausible, wrong answer.

The decision is that both halves are pinned by a test, and that the sharing
half is the one the documentation shows:

```rust
let interner = InternerContext::new();
let new_context = {
    let interner = interner.clone();          // an Arc clone: one interner
    move || ParseContext::<()> { interner: interner.clone(), ..Default::default() }
};
let syms = Cities::parse_FILE_pieces(input, new_context, Parallelism::Pieces(2))?;
// every symbol resolves against `interner`, in every piece
```

`tests/shared_interner_test.rs` holds this example and, next to it, the
fresh-per-piece case with its broken comparisons asserted on purpose — so
that a later change which makes ids meaningful across pieces, or which
rejects the mismatch, has to say so. `SYNTAX.md`'s `parse_FILE_pieces`
example keeps `ParseContext::default` for a grammar that returns no symbols,
but gains the sentence that names the condition.

Not decided here, because both cost more than the example does: a debug
assertion that pieces share an interner (`InternerContext` would need
identity, e.g. `Arc::ptr_eq` on the backend), and tying `Symbol` to its
interner in the type system. The second is the real fix and the expensive
one — it is what would turn a silent wrong answer into a compile error.

### 4. Why `parse_<rule>_pieces` takes a factory - the reason is not the interner

A claim worth correcting, because it makes the trap in §3 look designed away:
that `parse_<rule>_pieces` takes a *context factory* rather than a context "so
that the interner can be shared through an `Arc`, per ADR 14". The first half
is right - the parameter is `impl Fn() -> ParseContext<S> + Sync`, called once
per piece. The reason is not.

The factory exists because the rest of the context is **per-parse mutable
state**, not shared state: `furthest` (the recorded error), `rules` (the live
rule stack), `fold` (`FoldProgress`), `diagnose` and `user_state`. Every piece
parses through `&mut` on its own, concurrently, so they cannot have one
context between them. Sharing the interner is what the closure *permits* -
by cloning an `Arc` into each fresh context - not what it is for. §3 is the
evidence: the factory has been there since ADR 16 and nothing has ever shared
anything through it.

It is worth noticing that the factory is not forced by the sharing
requirement either - ADR 19 proposes taking it up on that. `ParseContext` is `Clone` and `S: Clone` is already a
bound, so an API taking one context and cloning it per piece would carry the
interner's `Arc` into every piece *by default*, and the trap in §3 would not
exist. What the factory buys instead is a `user_state` that starts fresh per
piece rather than being copied - a real choice, made silently. Not changed
here: the signature is public API, and the documented example plus the test
of §3 cost nothing and break nobody.

### 5. What is not decided here

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
* **Symbols are meaningful only against the interner that made them** — see
  §3, which this ADR promotes from a footnote to a decision because `intern`
  puts symbols in front of many more grammars than `ident` did.
* **Documentation debt this uncovered, paid with the implementation.**
  `README.md`'s return-type table said `ident` returns `String` (it returns
  `Symbol`) and `string` returns `String` (it borrows `&'a str`); both are
  corrected, and `raw_ident` and `intern` are listed beside them.
  `SYNTAX.md` gains an interning section: what a `Symbol` is, that `ident` is
  `intern(raw_ident)`, that `intern` normalises nothing, and that `_state` is
  where normalising belongs. `extern rule` remains undocumented - it works
  (a hand-written parser reaches `i.state.interner` like any generated one),
  and documenting it is its own change.
