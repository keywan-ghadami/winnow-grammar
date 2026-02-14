# Remaining Improvements

## High Priority (Core Features & Correctness)

1. [DONE] Integrate `winnow::error::ContextError` for better reporting.
2. Optimize usages of the cut operator.
3. Add `expect` combinator for clearer errors.
4. Implement synchronization points for recovery (refine `recover`).
5. Support user-defined error mapping functions.
6. Support `not` negative lookahead pattern.
7. Allow `where` clauses on grammar rules (for generics).
8. [DONE] Support rule visibility attributes like `pub`.
9. Map `winnow::stream::Location` to spans.
10. Detect infinite recursion loops during compilation.
11. Add diagnostics for grammar conflicts.

## Medium Priority (Optimization & Usability)

12. Optimize `alt` using `dispatch` combinator.
13. Use `preceded` for efficient whitespace skipping.
14. Inline small rules automatically for speed.
15. Support `#[inline]` attribute on rules.
16. Use `dispatch!` for faster keyword matching.
17. Optimize literal matching with `one_of`.
18. Deduplicate common prefixes in alternatives.
19. Generate lookup tables for character classes.
20. Profile generated code to find bottlenecks.
21. Benchmark against handwritten `winnow` parsers.
22. Support `Partial` input streams explicitly.
23. Support `Stateful` input streams in macros.
24. Support custom input types beyond strings.
25. Expose `winnow` combinators directly in grammar.
26. Support `checkpoint` and `reset` manually.
27. Bind to `winnow::token::any` combinator.
28. Support `winnow::token::eof` for end check.
29. Support custom whitespace parsers per rule.
30. Generate `Debug` impls for AST nodes.
31. Allow external rule imports with paths.
32. Support multiple grammar blocks in one file.
33. Generate parsing traces for debugging.
34. Support `#[cfg]` attributes on alternatives.
35. Generate `Display` impl for unparsing.
36. Add property-based testing generator.
37. Improve hygiene of generated rust code.

## Low Priority (Nice to Haves & Extras)

38. Generate `Visitor` trait for AST nodes.
39. Generate `Fold` trait for AST nodes.
40. Implement `fold` or `reduce` actions.
41. Support binary data parsing specific rules.
42. Add bit-level parsing support.
43. Support stateful parsing by passing context.
44. Allow mutable arguments in rules.
45. Support `impl` blocks for grammar state.
46. Add `@` binding for arbitrary expressions.
47. Support regex-like repetition ranges.
48. Create grammar debugger and visualizer tool.
49. Add LSP support for grammar files.
50. Generate railroad diagrams from grammar.
51. Add fuzzer integration for grammars.
52. Improve compile error messages for macros.
53. Add `trace` feature to print steps.
54. Validate left-recursion logic for edge cases.
55. Add examples for parsing JSON format.
56. Add examples for parsing TOML format.
57. Remove unused imports in generated code.
58. Simplify recursive loop generation logic.
59. Refactor `Codegen` struct for readability.
60. Standardize naming of generated functions.
61. Update `syn-grammar` dependency version.
62. Clean up `winnow-grammar-macro` dependencies.
63. Remove `allow(dead_code)` where possible.
64. Unify delimiter handling in codegen.
65. Abstract sequence generation logic further.
66. Improve handling of binding names.
67. Write a tutorial for complex expressions.
68. Document best practices for error recovery.
69. Create a cookbook of common patterns.
70. Add links to `winnow` documentation.
71. Document how to mix handwritten parsers.
72. Explain whitespace handling in depth.
73. Create a migration guide from `nom`.
74. Add badges to README file.
75. Setup CI/CD pipeline for project.
76. Add contributing guidelines for developers.
77. Support semantic actions without AST construction.
78. Allow async parsing rules in grammar.
79. Support streaming parsing mode explicitly.
80. Integrate with `ariadne` for error reports.
81. Support indentation-sensitive grammars.
82. Add macro for testing parsers inline.
83. Support precedence climbing helper utility.
84. Allow defining tokens separately from rules.
85. Support operator overloading syntax in rules.
86. Optimize codegen for `no_std` environments.
