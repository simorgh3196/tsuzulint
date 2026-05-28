# Embedding and distribution

> Status: template (M0). "Compiles to wasm" ≠ embeddable for free.

Three deliberate, semver'd surfaces:

1. **Rust library** (`tzlint_core` + a thin `tzlint` facade): a small stable embedding API
   (`Engine::lint(text, config) -> Diagnostics`), distinct from the plugin ABI.
2. **npm package via `wasm-bindgen`:** one artifact for **browser and Node**, ergonomic JS
   API. The "write once" surface for JS/TS embedders.
3. **Optional Node native addon (`napi-rs`):** only if the wasm path is too slow.

- **Editors integrate primarily via LSP** (M5); in-process embedding is for build tools,
  CI libraries, the playground, or tighter-than-LSP integration.
- **`Host` provider trait** abstracts the I/O the engine needs (file, dictionary fetch,
  cache, clock) so native / Node / browser each inject their environment. Keep
  `tzlint_core` free of native-only hard deps (it must compile to `wasm32`).
- **Dynamic plugins degrade gracefully** without COOP/COEP → fall back to the bundled
  canonical ruleset; the API exposes whether dynamic plugins are available.
