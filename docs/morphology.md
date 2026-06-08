# Morphology (ja / ko / zh)

> Status: template (M0/M2).

- **Provider abstraction:** `MorphologyProvider { lang(), analyze(text) -> TokenTable }`.
  A backend may cover several languages (e.g. ja/ko/zh from one engine); engines are pluggable.
- **Language-neutral `MorphologyV1`:** `Token { surface, lang, features: Vec<(k,v)>,
  reading: Option, base_form: Option }` — open features (the tokenizer's per-token detail list),
  optional fields present only where meaningful. Avoids a forced `V1→V2` bump across tagsets.
- **Dynamic dictionaries (not embedded):** fetched on demand from a hash-pinned,
  configurable source (local path / mirror allowed), cached (native FS / browser
  IndexedDB), compressed. Only languages required by enabled rules. Dictionary
  identity/version is in the cache key.
- **Licenses vary** per dictionary (IPADIC / UniDic / ko-dic / CC-CEDICT; CC-CEDICT is
  share-alike) — documented here; not embedded in the binary.

## Web delivery & dictionary loading

> Status: design intent (M2/M4). Measured: the code-only wasm payload is ~157 KiB brotli
> today (parser + config + engine); a Japanese dictionary is tens of MB. The dictionary —
> not the code — dominates web traffic, so it stays a separately cached artifact and is
> never embedded in the wasm.

- **Default stays non-embedded.** Bundling a dictionary into the wasm/npm artifact is
  rejected as the default: it forces tens of MB on every visitor (including dictionary-free
  rule sets), destroys cache granularity (a one-line code change would invalidate the whole
  multi-MB blob), blocks independent dictionary updates, and would pull per-dictionary
  licenses (e.g. share-alike CC-CEDICT) into the binary. A separate, hash-pinned,
  IndexedDB-cached artifact avoids all four.
- **Preload / warm-up is first-class.** The JS surface exposes an explicit
  `preloadDictionary(lang) -> Promise<void>` so embedders can fetch ahead of the first lint
  (during a splash screen or `requestIdleCallback`) instead of paying the latency inline.
  Preloading changes *timing*, not architecture — the artifact and cache key are unchanged.
- **Prefetch & offline hints.** Recommended patterns: `<link rel="prefetch">`
  (will-need-soon) / `rel="preload"` (need-now) for the dictionary URL, and a Service Worker
  precache for offline use. Second and later visits hit the IndexedDB cache and transfer zero
  bytes.
- **Dictionary size levers** (independent of delivery): prefer a compact base dictionary over
  expanded variants (plain IPADIC over NEologd; UniDic only when its precision is needed),
  ship compressed (zstd/gzip, decompressed client-side), trim features to those the enabled
  rules read, and offer a reduced "lite" dictionary preset for the playground.
- **Opt-in bundled variant** for offline / air-gapped / Electron / zero-config use is a
  sidecar package, never the default build — see [`embedding.md`](embedding.md).
