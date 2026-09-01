# Known Limitations

## Parser parameters in generic rules are not substituted

A rule may take a **type** parameter and a **parser** parameter:

```text
list<T>(item) -> Vec<T> = items:item* -> { items }
integers -> Vec<i32> = l:list(item=i32) -> { l }
```

The type parameter works. The parser parameter does not: `item` is not
substituted into the rule body, so the generated code refers to it as an
ordinary value and the compilation fails with

```text
error[E0425]: cannot find value `item` in this scope
```

Type parameters alone (`list<T>` without a parser parameter) are unaffected.

The example in [`SYNTAX.md`](SYNTAX.md) under *Generic Rules* is marked
`rust,ignore` for this reason — deliberately, and named here rather than hidden
behind a bare `FIXME` in the source.

`syn-grammar`, from which this crate was forked, does not have this defect; its
substitution pass (`monomorphize.rs`) is the reference for a fix.

## Whitespace rules must be lexical

A rule used by `WS` must be **UPPERCASE**, i.e. lexical:

```text
WSE = multispace1
WS  = (WSE | COMMENT)*
COMMENT = "//" until(line_ending)
```

A lowercase `comment` would be syntactic, so the generator inserts `WS` between
its own tokens — and `WS` calls the comment rule. That cycle recurses until the
stack is exhausted, with a bare `stack overflow` and no diagnostic.

The generator already computes cycles in `analysis::analyze_grammar`; reporting
this one at macro time instead of letting it overflow at run time would be the
obvious improvement.
