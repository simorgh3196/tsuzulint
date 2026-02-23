# tsuzulint_cli

TsuzuLint のコマンドラインインターフェース（CLI）アプリケーション。バイナリ名は `tzlint` です。

## 概要

`tsuzulint_cli` は、TsuzuLint プロジェクトの **コマンドラインインターフェース（CLI）アプリケーション** です。

**主な役割:**

- ユーザーとの中核的な対話ポイントを提供
- `tsuzulint_core` クレートをラップし、コマンドライン引数の解析と結果の出力を担当
- 設定ファイルの管理、プラグインのインストール、LSP サーバーの起動など、開発者ワークフローをサポート

## インストール

```bash
cargo install tsuzulint_cli
```

または、ソースからビルド:

```bash
git clone https://github.com/simorgh3196/tsuzulint
cd tsuzulint
cargo build --release --bin tzlint
```

## 使用方法

### グローバルオプション

| オプション | 短縮形 | 説明 |
| ---------- | ------- | ---- |
| `--config <PATH>` | `-c` | 設定ファイルのパスを指定 |
| `--verbose` | `-v` | 詳細なデバッグ出力を有効化 |
| `--no-cache` | | キャッシュを無効化 |

### サブコマンド

#### `lint` - ファイルのリント実行

```bash
tzlint lint [OPTIONS] <PATTERNS>...
```

| オプション | 説明 |
| ---------- | ---- |
| `--format <FORMAT>` | 出力形式（`text` / `json` / `sarif`）。デフォルト: `text` |
| `--fix` | 自動修正を適用 |
| `--dry-run` | 修正をプレビューのみ（`--fix` と併用必須） |
| `--timings` | パフォーマンス計測を表示 |
| `--fail-on-resolve-error` | ルール解決失敗時にエラーで終了 |

**使用例:**

```bash
# 基本的なリント
tzlint lint src/**/*.md

# JSON 形式で出力
tzlint lint --format json src/**/*.md

# 自動修正を適用
tzlint lint --fix src/**/*.md

# 修正をプレビュー
tzlint lint --fix --dry-run src/**/*.md

# パフォーマンス計測付き
tzlint lint --timings src/**/*.md

# 複数パターン
tzlint lint "src/**/*.md" "docs/**/*.md"
```

#### `init` - 設定ファイルの初期化

```bash
tzlint init [--force]
```

- `.tsuzulint.jsonc` ファイルを作成
- `--force`: 既存ファイルを上書き

**生成されるファイル:**

```jsonc
{
  // TsuzuLint 設定ファイル
  "rules": [
    // ここにルールを追加
    // "owner/repo",
    // { "github": "owner/repo@v1.0", "as": "alias" }
  ],
  "options": {
    // ルールごとのオプション
  }
}
```

#### `rules` - ルール管理

```bash
tzlint rules create <NAME>   # 新規ルールプロジェクト作成
tzlint rules add <PATH>      # WASM ルールを追加
```

**ルールプロジェクト作成:**

```bash
tzlint rules create my-rule
cd my-rule
cargo build --release --target wasm32-wasip1
```

生成される構造:

```text
my-rule/
├── Cargo.toml       # wasm32-wasip1 ターゲット用設定
└── src/
    └── lib.rs       # ルールテンプレート
```

#### `lsp` - LSP サーバー起動

```bash
tzlint lsp
```

エディタ統合用の Language Server Protocol サーバーを起動します。

#### `plugin` - プラグイン管理

```bash
tzlint plugin cache clean                          # キャッシュクリア
tzlint plugin install [SPEC] [--url <URL>] [--as <ALIAS>]  # プラグインインストール
```

**インストール例:**

```bash
# GitHub から
tzlint plugin install owner/repo

# 特定バージョン
tzlint plugin install owner/repo@v1.0.0

# エイリアス付き
tzlint plugin install owner/repo --as my-rule

# URL から
tzlint plugin install --url https://example.com/rule.wasm --as external-rule
```

## 出力フォーマット

### Text 形式（デフォルト）

```text
/path/to/file.md:
  0:10 error [rule-id]: エラーメッセージ

Checked 5 files (2 from cache), found 3 issues
```

`--timings` オプション有効時:

```text
Performance Timings:
Rule                            | Duration        | %
--------------------------------+-----------------+----------
no-todo                         | 15ms            | 45.0%
spell-check                     | 10ms            | 30.0%
--------------------------------+-----------------+----------
Total                           | 33ms
```

### JSON 形式

```json
[
  {
    "path": "/path/to/file.md",
    "diagnostics": [
      {
        "rule_id": "no-todo",
        "message": "Found TODO",
        "severity": "error",
        "span": { "start": 0, "end": 4 }
      }
    ]
  }
]
```

### SARIF 形式

SARIF（Static Analysis Results Interchange Format）は、GitHub Advanced Security などで使用される標準形式です。

```bash
tzlint lint --format sarif src/**/*.md > results.sarif
```

## 終了コード

| コード | 意味 |
| ------ | ---- |
| `0` | 成功（エラーなし） |
| `1` | リントエラーあり |
| `2` | 内部エラー（設定エラー、無効な glob 等） |

## エディタ統合

### VS Code

1. LSP サーバーを起動する拡張機能を作成
2. `tzlint lsp` コマンドを使用

### Neovim

```lua
local lspconfig = require('lspconfig')
lspconfig.tsuzulint.setup {
  cmd = { "tzlint", "lsp" },
  filetypes = { "markdown" },
}
```

## tsuzulint_core との統合

CLI は以下のコンポーネントを `tsuzulint_core` から利用:

| コンポーネント | 用途 |
| ------------- | ---- |
| `Linter` | メインのリントエンジン |
| `LinterConfig` | 設定ファイルの読み込みと管理 |
| `LintResult` | リント結果の格納 |
| `Severity` | 診断の重要度 |
| `apply_fixes_to_file` | 自動修正の適用 |
| `generate_sarif` | SARIF 形式での出力生成 |

## セキュリティ機能

- **シンボリックリンク保護**: `init` コマンドおよびプラグインインストール時の設定ファイル更新で、シンボリックリンクを検出して書き込みを拒否
- **Unix システムでの保護**: `O_NOFOLLOW` フラグを使用して、シンボリックリンク経由での意図しないファイル上書きを防止

## 依存関係

| 依存クレート | 用途 |
| ----------- | ---- |
| `clap` | コマンドライン引数解析 |
| `miette` | ユーザーフレンドリーなエラー表示 |
| `tracing` | 構造化ロギング |
| `serde_json` | JSON 出力と設定ファイルの処理 |
| `jsonc-parser` | JSONC 設定ファイルの解析 |
| `tokio` | 非同期ランタイム |
| `tsuzulint_core` | コアリンター機能 |
| `tsuzulint_lsp` | LSP サーバー実装 |
| `tsuzulint_registry` | プラグインレジストリ |
