# Morphology (Japanese; ko/zh planned)

> Status: **shipped for Japanese (IPADIC)** — on the native CLI/LSP, and in the wasm build's
> `morphology` feature. The model reserves Korean and Chinese, but they are not wired yet (see
> [Languages](#languages)).

Morphology-dependent rules (e.g. [`no-doubled-joshi`](rule-development.md)) need a tokenized,
part-of-speech-tagged view of the text. TsuzuLint produces that through a backend-agnostic
**provider seam**, fed by a **dynamic, non-embedded dictionary**.

## The model

- **`MorphologyProvider`** (`tzlint_pdk`) — the seam a tokenizer backend implements:
  `lang() -> Lang` and `analyze(text, base_offset, node) -> Result<MorphologyV1, _>`. It is
  `no_std`/`alloc` and names no concrete tokenizer; backends are injected Host-style, so the
  core stays dictionary-free and `wasm32`-clean by default.
- **`MorphologyV1`** (`tzlint_ast`) — a frozen, additive table keyed by node. Each `Token`
  carries `surface`, `lang`, `tagset`, optional `reading`/`base_form`, and **open `(key, value)`
  feature pairs**. The open `FeatureKey` (a bare `u32`) is deliberate: a new dictionary scheme
  emits its own columns without forcing a `V1 → V2` bump. An `is_unknown` flag marks
  out-of-vocabulary runs the analyzer guessed rather than looked up.
- **The lindera backend** (`tzlint_morphology_native`) is the shipped provider. It is
  target-agnostic (native **and** `wasm32`) but lives outside the morphology seam, so the seam
  crates (`tzlint_ast`/`tzlint_pdk`/`tzlint_core`) never depend on it — enforced by a CI
  dependency-tree check. It maps IPADIC's `details()` columns to the canonical feature keys.

The analysis pass runs only the providers whose language an enabled rule actually requires, and
folds the result into one `MorphologyV1` per document for the engine. The registry and the
node-keyed table are described in
[`design/morphology-registry-and-fingerprint.md`](design/morphology-registry-and-fingerprint.md).

## Dictionaries (dynamic, not embedded)

Dictionaries are **provisioned at runtime**, never compiled into the binary:

- The source is a **hash-pinned, compressed container** (`.dict.zst`). On use it is verified
  against `pin` — a BLAKE3 hash over the **compressed** bytes — **before** decompression, then
  decompressed in memory and assembled into a lindera `Dictionary`. A wrong pin, malformed pin,
  or undecodable container is an error, never a panic.
- It is **cached** keyed by the pin: on the native CLI under `.tzlint/dict/` in the working
  directory; in the browser the JS host owns the cache (IndexedDB/OPFS). The dictionary's
  identity (its pin) is part of the document cache key, so a dictionary upgrade invalidates
  stale cached diagnostics.
- It is provisioned **only when a rule active for the current run needs that dictionary's
  language** — a run that lints no Japanese-rule input does no network or disk work, and an
  unconfigured run keeps a cache key byte-identical to a pre-morphology run.

The container is a positional, self-describing blob (no per-member name strings); its format and
the in-memory bridge are documented in
[`design/dictionary-container-and-cli-wiring.md`](design/dictionary-container-and-cli-wiring.md).
Maintainers build one from lindera's embedded IPADIC with the `pack_ipadic` example, compress it
(`zstd`), and publish its `b3sum` as the config `pin`.

### Configuration

```jsonc
{
  "morphology": {
    "path": "dict/ipadic.dict.zst",   // local container (or "url" for an https source)
    "pin":  "<64 hex BLAKE3 over the compressed container>",
    "lang": "ja"                       // only "ja" is supported today
  }
}
```

Exactly one of `path`/`url`; `url` is `https`-only and SSRF-guarded. See the
[configuration reference](config-reference.md#morphology--dictionary-for-morphology-dependent-rules)
for the full surface.

## Web delivery & loading

> The code-only wasm payload is small (~157 KiB brotli, measured: parser + config + engine); a
> Japanese dictionary is tens of MB. The dictionary — not the code — dominates web traffic, so it
> stays a separately cached artifact and is **never** embedded in the wasm.

- **Two artifacts, chosen at build time.** The default **lean** build is tokenizer-free and tiny;
  morphology rules stay inert. The **full** build (`--features morphology`) bundles the Japanese
  backend so those rules fire with the same analysis as the CLI. The feature is named for the
  capability, not the backend, and is language-neutral — the language is chosen at runtime by the
  dictionary you register, not at compile time.
- **`registerDictionary(lang, compressedBytes, pinHex)`** is the JS surface. The host owns the
  fetch **and** the IndexedDB/OPFS cache; wasm only verifies the pin and decompresses the bytes it
  is handed (the same container/pin pipeline as the CLI). This keeps fetch policy, caching, and
  offline strategy in the embedder's hands.
- **Warm up by fetching ahead of the first lint.** Because the host owns the fetch, an app can
  load + `registerDictionary` during idle/splash time instead of paying the latency inline. Pairs
  with `<link rel="prefetch">`/`rel="preload"` for the dictionary URL and a Service Worker
  precache for offline use; second and later visits hit the cache and transfer zero bytes.
- **Dictionary size levers** (independent of delivery): prefer a compact base dictionary over
  expanded variants (plain IPADIC over NEologd; UniDic only when its precision is needed), ship
  compressed, trim features to those the enabled rules read, and offer a reduced **"lite"
  dictionary** for the playground.
- **An opt-in bundled variant** (a sidecar package / all-in-one build) for offline / air-gapped /
  Electron / zero-config use is never the default — see [`embedding.md`](embedding.md).

## Languages

- **Japanese (IPADIC)** ships today and powers `no-doubled-joshi`.
- **Korean and Chinese are reserved, not yet wired.** The frozen model already carries
  `Lang::{KO, ZH}` and `Tagset::{KO_DIC, CC_CEDICT}`, and features are open, so adding them is
  **additive** (no ABI bump). lindera serves ja/ko/zh from one engine; what remains is a
  per-scheme `analyze` column mapping (ko-dic / CC-CEDICT columns differ from IPADIC), deriving
  the `Lang`/`Tagset` from the dictionary's metadata instead of the current IPADIC hard-coding,
  relaxing the `"ja"`-only config/wasm guards, packing the dictionaries, and — the real work —
  designing the Korean/Chinese rules that consume the tokens.
- **UniDic** (`Tagset::UNIDIC`) is reserved but unexercised; Japanese ships on IPADIC.

## Licenses

The tokenizer **engine** (lindera, **MIT**) is the only morphology code in the binary/wasm.
**Dictionaries are not embedded** — each is fetched or packed separately under its own license,
so a share-alike dictionary (CC-CEDICT) never attaches its terms to the artifact:

| Dictionary | Language | License (summary) |
| --- | --- | --- |
| IPADIC | ja | BSD-style (IPADIC license) |
| UniDic | ja | tri-license (GPL / LGPL / BSD) |
| mecab-ko-dic | ko | Apache-2.0 |
| CC-CEDICT | zh | CC BY-SA (share-alike) |

These are summaries; consult each dictionary's upstream for exact terms before redistributing a
packed container. Because the dictionary travels as a separate artifact, redistributing one is a
decision the operator makes explicitly, with that dictionary's license in view.
