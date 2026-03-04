# winnow-grammar Architecture Notes

## structure
- **Front:** `syn-grammar-model` (AST).
- **Back:** `winnow-grammar` -> `winnow` parsers.
- **Macro:** `winnow-grammar-macros/src/codegen/mod.rs`.

## codegen logic
- **Modes:** 
  - `Syntactic` (`!is_lexical`): Implicit `ws` between steps.
  - `Lexical` (`is_lexical`): Strict, no implicit `ws`.
- **Whitespace (`ws`):**
  - Defaults to `multispace0`. Can be overridden by rule `ws`.
  - **Wrapper:** Public rules get `parse_{name}` wrapper: `ws?` -> `inner` -> `ws?` -> `eof`.
  - **Sequences:** `step1` -> `ws` -> `step2`.
  - **Delimiters:** `open` -> `ws` -> `inner` -> `ws` -> `close`.

## quantifier handling (`*`, `+`, `Count`)
- **Issue:** `winnow::separated` panics if separator matches empty (e.g., `multispace0`).
- **Fix:** Use `repeat(..., preceded(ws, item))` in syntactic mode.
  - Allows optional separator.
  - Consistent with `separated` semantics for optional `ws`.
  - **Lexical:** Uses `repeat` directly.

## spans & offsets
- **Wrapper effect:** Wrapper consumes leading `ws`. Inner rule starts *after* `ws`.
- **Spans:** `t:term @ s` -> `s` excludes leading whitespace consumed by wrapper.
- **EOF:** Syntactic `item eof` -> `item` -> `ws` -> `eof`. Input `item <space>` succeeds.

## primitives
- `u32`, `i32` etc map to `winnow::ascii::dec_uint/int`.
- `ident`, `string`, `char` have custom winnow implementations in codegen.
