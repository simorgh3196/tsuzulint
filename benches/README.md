# benches/

TsuzuLint のベンチマーク一式です。textlint との wall-time 比較を再現可能な形で提供します。

詳細な数値と計測方法は [`docs/benchmarks.md`](../docs/benchmarks.md) を参照してください。

## クイックスタート

```bash
# 必須: hyperfine
brew install hyperfine    # または: cargo install hyperfine

# 一発セットアップ (release ビルド + WASM ルール + textlint 依存 + コーパス生成)
bash benches/scripts/setup.sh

# 全サイズで比較 (small / medium / large / monolithic)
bash benches/scripts/compare.sh

# サイズ指定
bash benches/scripts/compare.sh medium
```

結果は `benches/results/<size>.{md,json}` に書き出されます。

## ディレクトリ構成

```text
benches/
├── README.md                      # このファイル
├── corpus/                        # 計測用コーパス (スクリプトで生成)
│   ├── seed.md                    # 元になる日本語 Markdown
│   ├── small/      (1 file)
│   ├── medium/    (10 files)
│   ├── large/    (100 files)
│   └── monolithic/ (1 file, 2MB)
├── configs/
│   ├── tsuzulint-native/          # native rule engine 用設定
│   ├── tsuzulint/                 # WASM rule engine 用設定 (現状は未使用)
│   └── textlint/                  # 対比対象の textlint 設定と npm deps
├── scripts/
│   ├── gen-corpus.sh              # seed.md からコーパス生成
│   ├── setup.sh                   # 一発セットアップ
│   └── compare.sh                 # hyperfine 比較
└── results/                       # 最新の計測結果 (MD + JSON)
```

## 手元で編集する場合

- **コーパスを大きくしたい**: `benches/scripts/gen-corpus.sh` の `seq` 範囲を編集
- **ルールを揃えて比較したい**: `configs/tsuzulint-native/.tsuzulint.jsonc` と
  `configs/textlint/.textlintrc.json` を同期
- **実行回数を増やしたい**: `RUNS=20 bash benches/scripts/compare.sh`

## 計測哲学

- **Wall time を基準**: 開発者体験で効く指標はキャッシュを含めた一回の実行時間
- **再現可能性重視**: コーパスはスクリプトで生成 (大きいファイルをリポジトリに入れない)
- **公平性**: 同じコーパスに同等ルール、同じサンドボックス、同じ `hyperfine --warmup`

## 既知の制限

- `benches/configs/tsuzulint/` (WASM 版) は現状のディスパッチ問題 (ノードタイプ
  フィルタが効いていない) により diagnostic が出ないため、計測から除外しています。
  Native Rule Engine 側で性能証明を先にやり、WASM 側は別途デバッグする方針です。
