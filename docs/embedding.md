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

## Dictionary distribution (web)

> Status: the runtime-fetch model below is **shipped** for Japanese (the wasm `morphology` build
> plus `registerDictionary`); the opt-in bundled sidecar is still design intent. The code-only
> wasm is light (~157 KiB brotli, measured); the morphology dictionary (tens of MB for Japanese)
> is the only real bandwidth term and is delivered separately. See [`morphology.md`](morphology.md)
> for the dictionary model.

- **Default: dictionary fetched at runtime, never embedded.** Hash-pinned, compressed,
  IndexedDB-cached, per-enabled-language. The npm/wasm artifact ships code only, so a code
  update never re-downloads the dictionary and a dictionary update never re-downloads the
  code. Rule sets that need no morphology fetch no dictionary at all.
- **The JS host owns the fetch, via `registerDictionary(lang, compressedBytes, pinHex)`.** wasm
  verifies the pin and decompresses the bytes it is handed; the host fetches and caches them.
  Because the host owns the fetch, warming up is just loading + registering during idle/splash
  time rather than on the first lint. Pairs with `<link rel="prefetch">` and a Service Worker
  precache (recommended patterns, not requirements).
- **Opt-in bundled variant** (e.g. a `@tsuzulint/dict-ja-*` sidecar package, or an
  all-in-one build) for offline / air-gapped / Electron / zero-config embedders. It is an
  explicit opt-in, never the default: bundling into the main artifact would force the
  dictionary on every embedder, break cache granularity, and entangle dictionary licenses
  with the binary.
