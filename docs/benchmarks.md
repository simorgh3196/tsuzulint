# ベンチマーク: TsuzuLint vs textlint

本ドキュメントは TsuzuLint の性能を textlint との比較で定量化するものです。
単位は壁時計時間 (wall clock)、計測は [hyperfine](https://github.com/sharkdp/hyperfine) を使用。

## 計測条件

| 項目 | 値 |
| :--- | :--- |
| マシン | Apple M シリーズ (開発者端末) |
| OS | macOS (darwin) |
| tsuzulint | `target/release/tzlint` (Native Rule Engine 使用) |
| textlint | v15.5.4 (Node.js v24.12.0) |
| 計測 | `hyperfine --warmup 2 --runs 5` |
| 対象ルール | `no-todo`, `sentence-length` |

textlint 側の `.textlintrc.json` は `benches/configs/textlint/`、tzlint 側は
`benches/configs/tsuzulint-native/.tsuzulint.jsonc` を使用。両者で同等ルールを
有効化している。

## コーパス

`benches/corpus/` 以下に配置。1ファイルあたり約 4KB の日本語 Markdown テンプレート
(`benches/corpus/seed.md`) を複製して生成している。

| サイズ | ファイル数 | 合計サイズ | 用途 |
| :--- | ---: | ---: | :--- |
| `small`      |   1 |    4 KB | 起動オーバーヘッドの計測 |
| `medium`     |  10 |  120 KB | 通常規模の中リポジトリ |
| `large`      | 100 |  1.2 MB | 典型的な技術ドキュメントプロジェクト |
| `monolithic` |   1 |  2.0 MB | 単一の超大型ファイル |

## 結果

### 2ルール同時 (`no-todo`, `sentence-length`)

| ケース | tsuzulint (native) | textlint | 倍率 |
| :--- | ---: | ---: | ---: |
| small      |  5.2 ms ± 0.2 |   681.8 ms ±  20.8 |   **132x** faster |
| medium     |  5.3 ms ± 0.4 |   859.4 ms ±   5.2 |   **161x** faster |
| large      |  7.4 ms ± 1.1 |  2130.2 ms ±  68.8 |   **288x** faster |
| monolithic |  5.7 ms ± 0.2 | 50508.9 ms ± 2608.0 | **8838x** faster |

### プリセット同時有効化 (`ja-technical-writing`)

tsuzulint 側はネイティブ10ルール (形態素解析を使う `no-doubled-joshi` 含む)、
textlint 側は公式 `preset-ja-technical-writing` (約20ルール) で同じコーパスを比較。

| ケース | tsuzulint (preset) | textlint (preset) | 倍率 |
| :--- | ---: | ---: | ---: |
| small  |  5.6 ms ± 0.2 |   841.1 ms ±   7.9 | **150x** faster |
| medium |  5.9 ms ± 0.6 |  1514.2 ms ±  23.0 | **256x** faster |
| large  |  7.2 ms ± 0.5 |  6896.7 ms ± 203.0 | **963x** faster |

形態素解析 (Lindera/IPADIC) がロードされても tsuzulint は ms オーダーで
完走する。textlint 側は形態素解析を JS 実装 (kuromojin) で行うため、preset に
形態素ルールが入るほど差が開く構造になっている。

原始データは `benches/results/*.json` を参照 (`preset-*.md` がプリセット版)。

## 注意事項・補足

- **WASM ルールパイプラインはベンチ対象外**: TsuzuLint は WASM と Native の
  デュアルエンジンだが、2026-04 時点で WASM ルールパイプラインには
  ディスパッチ上の問題があり (`is_node_type("Str")` で Paragraph を受け取って
  早期 return する) ユーザーコーパスに対して発火しない。これが解消されれば
  WASM vs textlint の比較も有意になる (現状でも起動コストではネイティブに近い
  はず)。追跡は `TODO: 起票予定`。
- **tzlint の時間がコーパス規模に対してほぼ線形でない**のは、ファイル I/O と
  AST 構築が圧倒的に軽量で、かつ rayon による並列処理が効いているため。
  オーダーとしては「起動 + 1ファイルあたり μs オーダーの処理」に支配される。
- **textlint の monolithic 時間 (50s) は極端**: 単一 2MB ファイルに対して
  いくつかのルールが O(n²) に振る舞うためと推定。複数の小ファイルに分けた方が
  textlint には有利。それでも large コーパスで 288x 差があり、実プロジェクトでの
  体感速度差はかなり大きい。

## 再現手順

```bash
# 必須ツール
brew install hyperfine           # または cargo install hyperfine

# ワンショットセットアップ (release ビルド + WASM ルール + 依存 + コーパス)
bash benches/scripts/setup.sh

# 全サイズで比較
bash benches/scripts/compare.sh

# 特定サイズだけ
bash benches/scripts/compare.sh medium
```

結果は `benches/results/<size>.md` と `benches/results/<size>.json` に保存
されます。

## 使用ルール (内訳)

両者とも以下のルールを有効化:

| ルール | tsuzulint | textlint |
| :--- | :--- | :--- |
| `no-todo`          | native (built-in) | `textlint-rule-no-todo@2.0.1` |
| `sentence-length`  | native (built-in) | `textlint-rule-sentence-length@5.2.1` |

将来、ルール数を増やしても tsuzulint の優位は保たれると見込んでいる。根拠:

1. tzlint の支配項はパーサとAST走査で、ルール追加の限界コストは実質ゼロに近い
2. ネイティブルールのサブ ms の実行時間を textlint の起動 (~500ms) が吸収することは不可能
3. `rayon` 並列化により、ファイル数が増えるほど差が広がる
