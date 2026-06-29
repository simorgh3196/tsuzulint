# TsuzuLint

**日本語** | [English](README.en.md)

> **ステータス: プレリリース（0.1.0 開発中）。** 日本語リントの中核は完成済みです — 23ルールの
> `textlint-rule-preset-ja-technical-writing` と完全パリティ（組み込みルールは合計26）、形態素解析
> ベースの文体ルール、`prh` 相当の表記ゆれ/用語ルールを搭載。残る 0.1.0 の作業は VSCode 拡張とリリース
> パッケージングです。マイルストーンは [`docs/roadmap.md`](docs/roadmap.md) を参照してください。

**TsuzuLint** は Rust 製の高速な自然言語リンターで、**日本語版 `textlint` の置き換え**を目指しています
（韓国語・中国語も将来対応予定）。`ja-technical-writing` プリセット全体で Node 版 `textlint` の約 **15倍
高速**、メモリ使用量も大幅に少なく動作します（[ベンチマーク](docs/benchmarks.md)）。さらに**移行しやすい**
設計です: ルール名は textlint に意図的に揃えてあり、既存の `prh` `.prh.yml` 辞書をそのまま読み込めます
（[textlint からの移行](docs/migration-from-textlint.md)）。

- **ブランド:** TsuzuLint。**コマンド / クレート:** `tzlint`（`tzlint_*`）。長いブランド名に短いコマンド
  名を割り当てるのは一般的な慣習です（例: Visual Studio Code → `code`）。
- **目標:** 実行速度（インデックスベース AST、ゼロコピーのプラグイン読み取り、単一走査スケジューラ）、
  移植性（ネイティブ + `wasm32`、ブラウザを第一級の設計対象に）、容易なルール拡張（現在は Rust PDK、
  将来は TS/AssemblyScript 向けの階層化 ABI）、安全な破壊的変更（凍結された AST コア + 追加専用テーブル
  + `bytecheck`）、充実したテストとドキュメント。

## 使い方

[`tzlint` をインストール](docs/install.md)してください — 現時点ではソースからビルドします
（`cargo install --git …` または `cargo build --release` → `target/release/tzlint`）。プリビルドバイナリ・
npm ラッパー・エディタ拡張は 0.1.0 で提供予定です。その後:

```sh
# ファイル・ディレクトリ（Markdown は再帰探索）・glob をリント。`-` は標準入力を読む。
tzlint lint README.md docs/ 'src/**/*.md'
cat draft.md | tzlint lint -

# 出力フォーマットを選択（text | json | sarif）。
tzlint lint --format json docs/
tzlint lint --format sarif docs/ > results.sarif   # 例: GitHub code scanning

# 自動修正をその場で適用（--dry-run でプレビュー）。`fix -` は標準入力→標準出力。
tzlint fix docs/
tzlint fix --dry-run docs/
cat draft.md | tzlint fix - > fixed.md

# 作業ディレクトリに雛形 .tzlintrc.json を書き出す。
tzlint init

# 解決後のルールセットを確認（--config / 設定探索を尊重）。
tzlint rules list
tzlint rules explain max-ten
```

ディレクトリ引数は `.md`/`.markdown` を再帰的に探索します（隠しエントリとシンボリックリンクはスキップ）。
glob（`*`, `?`, `[...]`, `**`）は厳密にマッチするため、シェルが先に展開しないようクォートしてください。

グローバルオプション: `-c/--config <PATH>`（上位探索の代わりに指定の設定ファイルを使用）、`-v/--verbose`
（追加の注記を stderr へ）、`--no-cache`（ドキュメントキャッシュを無効化）。

`lint` は問題なしで `0`、診断が1件以上で `1`、運用エラー（不正な設定、読み取り不能なファイルなど）で `2`
を返します — CI 向けの慣例的な終了コードです。text フォーマットは `path:line:col: severity: message [rule]`
で、`col` は診断開始位置の1始まりの桁です:

```text
$ printf 'これはﾊﾛｰという文です。\n' | tzlint lint -
<stdin>:1:4: warning: 半角カタカナは推奨されません。全角カタカナを使ってください。 [no-hankaku-kana]
1 file(s) checked, 1 issue(s) found
```

各ドキュメントへのリンク（いずれも英語）: 設定は [`docs/config-reference.md`](docs/config-reference.md)
（全ルール一覧も）、textlint からの移行は [`docs/migration-from-textlint.md`](docs/migration-from-textlint.md)、
インストール方法は [`docs/install.md`](docs/install.md)、`--format json` の仕様は
[`docs/json-output.md`](docs/json-output.md)、CSV/TSV の列リントは [`docs/processors.md`](docs/processors.md)、
textlint との性能比較は [`docs/benchmarks.md`](docs/benchmarks.md) を参照してください。

## ワークスペース構成

```
crates/
  tzlint_ast    凍結 ABI 型（インデックスベース AST、Span）
  tzlint_core   パーサ + リントエンジン + 設定 + キャッシュ + io
  tzlint_rules  組み込みネイティブルール
  tzlint_pdk    ルール作成者向け SDK
  tzlint_abi    共有メモリのプラグイン ABI
  tzlint_cli    `tzlint` バイナリ
  tzlint_lsp    LSP サーバ（v1 では雛形）
```

## ビルド

```sh
cargo build          # または: just build
cargo test           # または: just test
just check           # rustfmt + clippy + tests（CI と同等）
```

MSRV: Rust **1.94** — ローリングポリシー: **最新安定版 − 2**（wasmtime の「直近3つの安定版」に追従）。
Rust のリリースごとに更新します。開発は最新安定版で行います。
ライセンス: **Apache-2.0**。

ドキュメント（[`docs/`](docs/)）と Rustdoc は英語で記述されています。この README の英語版は
[README.en.md](README.en.md) を参照してください。
