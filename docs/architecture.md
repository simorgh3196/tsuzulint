# Architecture

> Status: template (M0). Filled in as crates land.

- **Crates (7):** `tzlint_ast`, `tzlint_core`, `tzlint_rules`, `tzlint_pdk`, `tzlint_abi`,
  `tzlint_cli`, `tzlint_lsp`. PDK-public boundary = `tzlint_ast` + `tzlint_abi` + `tzlint_pdk`.
- **Index-based AST:** `Ast { nodes: Vec<Node>, text: String, root: NodeId }`; `Span` is an
  **absolute** byte range into `Ast.text`. No lifetimes, contiguous, archives cleanly.
- **Dispatch:** one `Engine::lint(ast, rules)`. Single-traversal multi-visitor for native
  rules; plugins get one per-file hand-off and self-traverse.
- **Parser:** markdown-rs (`markdown`) 1.0 (mdast → index-AST transform). Alternatives
  (comrak / pulldown-cmark / tree-sitter-md) and the CST/incremental trade-off: see below.
- **Data flow & position mapping, text handling (encoding/normalization/columns),
  determinism:** TODO.
