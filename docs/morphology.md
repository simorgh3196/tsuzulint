# Morphology (ja / ko / zh)

> Status: template (M0/M2).

- **Provider abstraction:** `MorphologyProvider { lang(), analyze(text) -> TokenTable }`.
  A backend may cover several languages (lindera: ja/ko/zh); other engines allowed.
- **Language-neutral `MorphologyV1`:** `Token { surface, lang, features: Vec<(k,v)>,
  reading: Option, base_form: Option }` — open features (à la lindera `token.details()`),
  optional fields present only where meaningful. Avoids a forced `V1→V2` bump across tagsets.
- **Dynamic dictionaries (not embedded):** fetched on demand from a hash-pinned,
  configurable source (local path / mirror allowed), cached (native FS / browser
  IndexedDB), compressed. Only languages required by enabled rules. Dictionary
  identity/version is in the cache key.
- **Licenses vary** per dictionary (IPADIC / UniDic / ko-dic / CC-CEDICT; CC-CEDICT is
  share-alike) — documented here; not embedded in the binary.
