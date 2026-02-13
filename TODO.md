# Remaining Improvements

## High Priority (Core Features & Correctness)

1. Add support for user-defined custom error types.
2. Integrate `winnow::error::ContextError` for better reporting.
3. Optimize usages of the cut operator.
4. Add `expect` combinator for clearer errors.
5. Implement synchronization points for recovery (refine `recover`).
6. Support user-defined error mapping functions.
7. Support `separated_list` patterns (crucial for argument lists, etc.).
8. Implement `peek` pattern for lookahead.
9. Support `not` negative lookahead pattern.
10. Allow `where` clauses on grammar rules (for generics).
11. Support parameterized rules with generics.
12. Support rule visibility attributes like `pub`.
13. Map `winnow::stream::Location` to spans.
14. Detect infinite recursion loops during compilation.
15. Add diagnostics for grammar conflicts.

## Medium Priority (Optimization & Usability)

16. Optimize `alt` using `dispatch` combinator.
17. Use `preceded` for efficient whitespace skipping.
18. Inline small rules automatically for speed.
19. Support `#[inline]` attribute on rules.
20. Use `dispatch!` for faster keyword matching.
21. Optimize literal matching with `one_of`.
22. Deduplicate common prefixes in alternatives.
23. Generate lookup tables for character classes.
24. Profile generated code to find bottlenecks.
25. Benchmark against handwritten `winnow` parsers.
26. Support `Partial` input streams explicitly.
27. Support `Stateful` input streams in macros.
28. Support custom input types beyond strings.
29. Expose `winnow` combinators directly in grammar.
30. Support `checkpoint` and `reset` manually.
31. Bind to `winnow::token::any` combinator.
32. Support `winnow::token::eof` for end check.
33. Support custom whitespace parsers per rule.
34. Generate `Debug` impls for AST nodes.
35. Allow external rule imports with paths.
36. Support multiple grammar blocks in one file.
37. Generate parsing traces for debugging.
38. Support `#[cfg]` attributes on alternatives.
39. Generate `Display` impl for unparsing.
40. Add property-based testing generator.
41. Improve hygiene of generated rust code.

## Low Priority (Nice to Haves & Extras)

42. Generate `Visitor` trait for AST nodes.
43. Generate `Fold` trait for AST nodes.
44. Implement `fold` or `reduce` actions.
45. Support binary data parsing specific rules.
46. Add bit-level parsing support.
47. Support stateful parsing by passing context.
48. Allow mutable arguments in rules.
49. Support `impl` blocks for grammar state.
50. Add `@` binding for arbitrary expressions.
51. Support regex-like repetition ranges.
52. Create grammar debugger and visualizer tool.
53. Add LSP support for grammar files.
54. Generate railroad diagrams from grammar.
55. Add fuzzer integration for grammars.
56. Improve compile error messages for macros.
57. Add `trace` feature to print steps.
58. Validate left-recursion logic for edge cases.
59. Add examples for parsing JSON format.
60. Add examples for parsing TOML format.
61. Remove unused imports in generated code.
62. Simplify recursive loop generation logic.
63. Refactor `Codegen` struct for readability.
64. Standardize naming of generated functions.
65. Update `syn-grammar` dependency version.
66. Clean up `winnow-grammar-macro` dependencies.
67. Remove `allow(dead_code)` where possible.
68. Unify delimiter handling in codegen.
69. Abstract sequence generation logic further.
70. Improve handling of binding names.
71. Write a tutorial for complex expressions.
72. Document best practices for error recovery.
73. Create a cookbook of common patterns.
74. Add links to `winnow` documentation.
75. Document how to mix handwritten parsers.
76. Explain whitespace handling in depth.
77. Create a migration guide from `nom`.
78. Add badges to README file.
79. Setup CI/CD pipeline for project.
80. Add contributing guidelines for developers.
81. Support semantic actions without AST construction.
82. Allow async parsing rules in grammar.
83. Support streaming parsing mode explicitly.
84. Integrate with `ariadne` for error reports.
85. Support indentation-sensitive grammars.
86. Add macro for testing parsers inline.
87. Support precedence climbing helper utility.
88. Allow defining tokens separately from rules.
89. Support operator overloading syntax in rules.
90. Optimize codegen for `no_std` environments.
