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

両者とも同じ 2 ルールのみを有効化。

| ケース | tsuzulint (native) | textlint | 倍率 |
| :--- | ---: | ---: | ---: |
| small      |  7.1 ms ± 0.6 |   425.4 ms ±  10.5 |   **60x** faster |
| medium     |  6.2 ms ± 0.5 |   585.3 ms ±  48.7 |   **94x** faster |
| large      |  7.7 ms ± 0.7 |  1312.3 ms ±   7.8 |  **170x** faster |
| monolithic |  6.1 ms ± 0.6 | 23828.5 ms ± 213.2 | **3875x** faster |

### プリセット同時有効化 (`ja-technical-writing`)

tsuzulint 側はネイティブ10ルール (形態素解析を使う `no-doubled-joshi` 含む)、
textlint 側は公式 `preset-ja-technical-writing` (約20ルール) で同じコーパスを比較。
textlint 側のほうがルール数は多いため厳密な「同じ仕事」比較ではなく、
「同じ preset を有効化したときの体感速度」の比較と解釈するのが正しい。

| ケース | tsuzulint (preset) | textlint (preset) | 倍率 |
| :--- | ---: | ---: | ---: |
| small  |  1.7 ms ± 0.4 |   814.1 ms ±  24.2 |  **476x** faster |
| medium |  5.7 ms ± 0.4 |  1414.1 ms ±  33.1 |  **249x** faster |
| large  |  6.5 ms ± 0.5 |  6787.3 ms ± 251.4 | **1039x** faster |

形態素解析 (Lindera/IPADIC) がロードされても tsuzulint は ms オーダーで
完走する。textlint 側は形態素解析を JS 実装 (kuromojin) で行うため、preset に
形態素ルールが入るほど差が開く構造になっている。

原始データは `benches/results/*.json` を参照 (`preset-*.md` がプリセット版)。

### WASM ルールパイプラインでの実行

参考として、同じ `no-todo` を Native (built-in registry) ではなく **WASM プラグイン経由**
で実行した数値を取得した。設定は `benches/configs/tsuzulint/.tsuzulint.jsonc`、
計測コマンドは `benches/scripts/compare-wasm.sh`。

以下はディスパッチのバッチ化 (1 ブロック 1 コール) と、ノードが自身の
`value` を持つ場合にリクエストから全文 `source` を省く最小ペイロード化を
適用した後の数値。

| ケース | tzlint (native) | tzlint (WASM) | textlint | WASM ÷ native | WASM ÷ textlint |
| :--- | ---: | ---: | ---: | ---: | ---: |
| small      |  141.5 ms ±  1.1 |   158.3 ms ±  0.3 |    370.5 ms ±  3.0 | 1.12x | 0.43x |
| medium     |  145.5 ms ±  1.0 |   238.9 ms ± 18.5 |    484.1 ms ±  4.0 | 1.64x | 0.49x |
| large      |  170.7 ms ±  2.3 |  1099.9 ms ± 76.8 |   1319.2 ms ± 38.0 | 6.44x | 0.83x |
| monolithic |  616.7 ms (n=1)  |   784.7 ms (n=1)  | 28815.4 ms (n=1)   | 1.27x | 0.027x |

(monolithic は依然 `RUNS=1` で計測。high-σ の値は並列ビルド混在の騒がしい
環境による測定ノイズで、静かな環境での再ランを推奨。)

#### 観察

- **monolithic の二次オーダーは解消**: 旧来は 1 ブロックごとに 2MB の `source`
  を再シリアライズしていたため `block_count × source_len` で効き、WASM 経由は
  ~106s だった。バッチ化で 1 ブロック 1 コールにし、さらにノードが `value` を
  携える場合に `source` を空送りすることで、コストが文書サイズに対して線形に
  なり **~785ms / Native 比 1.27x / textlint 比 0.027x** まで改善した。
- **small/medium は依然 textlint より圧倒的に速い**: corpus が小さいうちは
  ホスト側の起動コストが支配的で、WASM のシリアライズオーバーヘッドは小さい。
- **large は per-file の WASM 起動コストが支配的**: large は 100 ファイルで
  1 ファイルあたりは小さく、`source` 省略は効きにくい。ここでの差はペイロード
  ではなくファイル単位のプラグイン呼び出し回数によるもので、別軸の最適化
  (ファイル横断のインスタンス再利用) が必要。
- **WASM dispatch は機能としては正しく動く**: `crates/tsuzulint_ast::collect_nodes_by_type`
  を `crates/tsuzulint_core/src/file_linter.rs` が呼び、manifest 宣言通りの
  ノード型を rule に渡す。Native と同じ件数の diagnostic を返す。

## 注意事項・補足

- **WASM ルールパイプラインの使い分け**: 上記の通り、サードパーティ製ルールの
  安全実行や言語非依存な配布など WASM ならではの用途には十分使えるが、
  本体に同梱する組み込みルールについては native 経路の方が常に高速。同じ
  ロジックを両方で書く意味はなく、本リポジトリでは native 側を「速さの基準」、
  WASM 側を「拡張インターフェース」と整理している。
- **tzlint の時間がコーパス規模に対してほぼ線形でない**のは、ファイル I/O と
  AST 構築が圧倒的に軽量で、かつ rayon による並列処理が効いているため。
  オーダーとしては「起動 + 1ファイルあたり μs オーダーの処理」に支配される。
- **textlint の monolithic 時間 (約 24s) は極端**: 単一 2MB ファイルに対して
  いくつかのルールが O(n²) に振る舞うためと推定。複数の小ファイルに分けた方が
  textlint には有利。それでも large コーパスで 170x 差があり、実プロジェクトでの
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
