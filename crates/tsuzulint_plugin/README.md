# tsuzulint_plugin

WASM プラグインシステムを提供するクレート。ルールを WASM にコンパイルし、サンドボックス環境で安全に実行します。

## 概要

`tsuzulint_plugin` は、TsuzuLint の中核を担う **WASM プラグインシステム** です。このクレートは以下の役割を果たします：

- **WASM ベースのルール実行**: リントルールを WASM にコンパイルし、サンドボックス環境で安全に実行
- **プラグインのロード・管理**: WASM ファイルまたはバイト列からのルール読み込み、設定、アンロード
- **ホスト関数の提供**: ルール実行時に必要なコンテキスト情報の提供
- **診断情報の収集**: ルールから返される診断結果の集約

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────────────┐
│                      PluginHost                              │
│  (高レベル API)                                              │
│  - load_rule(), configure_rule(), run_rule()                │
│  - run_all_rules()                                          │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    RuleExecutor trait                        │
│  (バックエンド抽象化)                                         │
└─────────────────────────────────────────────────────────────┘
            │                           │
            ▼                           ▼
┌─────────────────────┐     ┌─────────────────────┐
│   ExtismExecutor    │     │    WasmiExecutor    │
│   (native feature)  │     │  (browser feature)  │
│                     │     │                     │
│ - wasmtime (JIT)    │     │ - Pure Rust         │
│ - 高速実行          │     │ - WASM-in-WASM      │
│ - CLI/サーバー向け  │     │ - ブラウザ向け       │
└─────────────────────┘     └─────────────────────┘
```

## プラグインインターフェース

各ルールは以下の 2 つの関数をエクスポートする必要があります：

```rust
// 1. ルールメタデータを返す（JSON）
fn get_manifest() -> String

// 2. リントを実行し診断結果を返す（Msgpack）
fn lint(input_bytes: &[u8]) -> Vec<u8>
```

### 入力データ構造

```rust
struct LintRequest {
    tokens: Vec<Token>,       // 形態素解析結果
    sentences: Vec<Sentence>, // 文境界情報
    node: T,                  // AST ノード（シリアライズ済み）
    source: &str,             // ソーステキスト
    file_path: Option<&str>,  // ファイルパス
}
```

### 出力データ構造

```rust
struct LintResponse {
    diagnostics: Vec<Diagnostic>, // 診断結果のリスト
}
```

## Extism vs wasmi の比較

| 特性 | Extism (native) | wasmi (browser) |
| ------ | --------------- | --------------- |
| **実行方式** | JIT コンパイル | インタプリタ |
| **基盤技術** | wasmtime | 純粋 Rust |
| **パフォーマンス** | 高速 | 低速 |
| **使用環境** | CLI、Tauri、サーバー | ブラウザ WASM (WASM-in-WASM) |
| **セキュリティ制御** | Extism Manifest | wasmi StoreLimiter |

## Feature Flags

```toml
[features]
default = ["native"]
native = ["dep:extism", "dep:extism-manifest"]   # Extism バックエンド
browser = ["dep:wasmi"]                           # wasmi バックエンド
test-utils = ["dep:wat"]                          # テストユーティリティ
rkyv = ["dep:rkyv", "tsuzulint_ast/rkyv"]         # 高速シリアライズ
```

**重要な制約:**

- `native` または `browser` のいずれかが必須（コンパイルエラーになる）
- 両方有効な場合は `native` が優先

## 主要な型

### RuleManifest（ルールメタデータ）

```rust
pub struct RuleManifest {
    pub name: String,                    // ルールID
    pub version: String,                 // セマンティックバージョン
    pub description: Option<String>,     // 説明
    pub fixable: bool,                   // 自動修正可能か
    pub node_types: Vec<String>,         // 対象ノード型
    pub isolation_level: IsolationLevel, // 分離レベル
    pub schema: Option<Value>,           // 設定の JSON Schema
}

pub enum IsolationLevel {
    Global,  // 文書全体を必要とするルール
    Block,   // ブロック単位で独立実行可能
}
```

### Diagnostic（診断結果）

```rust
pub struct Diagnostic {
    pub rule_id: String,          // ルールID
    pub message: String,          // メッセージ
    pub span: Span,               // バイト範囲
    pub loc: Option<Location>,    // 行/列位置
    pub severity: Severity,       // 重要度
    pub fix: Option<Fix>,         // 自動修正
}

pub enum Severity {
    Info,
    Warning,
    Error,  // デフォルト
}

pub struct Fix {
    pub span: Span,    // 置換範囲
    pub text: String,  // 置換テキスト
}
```

## セキュリティ機能

### リソース制限

- **メモリ**: 128MB 上限（DoS 防止）
- **CPU**: Fuel 制限（10億命令 = 無限ループ防止）
- **時間**: タイムアウト（5秒 = 応答なしルール防止）

### アクセス制御

- ネットワークアクセス: 完全拒否
- ファイルシステムアクセス: 完全拒否
- 環境変数: クリア

## 使用例

```rust
use tsuzulint_plugin::PluginHost;

// ホストの作成
let mut host = PluginHost::new();

// ルールのロード
host.load_rule("./rules/no-todo.wasm")?;

// ルールの設定（オプション）
host.configure_rule("no-todo", serde_json::json!({
    "allow": ["TODO", "FIXME"]
}))?;

// ルールの実行
let diagnostics = host.run_rule(
    "no-todo",
    &ast_node,
    "source content",
    &tokens_json,
    &sentences_json,
    Some("example.md")
)?;

// 全ルールの一括実行
let all_diagnostics = host.run_all_rules(
    &ast_node,
    "source content",
    &tokens_json,
    &sentences_json,
    Some("example.md")
)?;

// 結果の処理
for diag in diagnostics {
    println!("{}: {} at {:?}", diag.rule_id, diag.message, diag.span);
}

// ルールのアンロード
host.unload_rule("no-todo");
```

## 依存関係

| 依存関係 | 用途 |
| ---------- | ------ |
| `extism` (optional) | ネイティブ環境での WASM 実行 |
| `extism-manifest` (optional) | Extism プラグイン設定 |
| `wasmi` (optional) | ブラウザ環境での WASM 実行（インタプリタ） |
| `serde` / `serde_json` | JSON シリアライズ |
| `rmp-serde` | MessagePack シリアライズ（高速化） |
| `thiserror` | エラー型定義 |
| `tracing` | ログ出力 |
| `tsuzulint_ast` | AST 型定義 |
| `tsuzulint_text` | Token/Sentence 型定義 |

**なぜ MessagePack を使用するか:**

- JSON より高速なシリアライズ/デシリアライズ
- よりコンパクトなバイナリ形式
- ホストと WASM 間のデータ転送を最適化

## モジュール構成

```text
src/
├── lib.rs              # クレートエントリーポイント
├── executor.rs         # RuleExecutor トレイト
├── executor_extism.rs  # Extism バックエンド（native feature）
├── executor_wasmi.rs   # wasmi バックエンド（browser feature）
├── host.rs             # PluginHost（高レベル API）
├── manifest.rs         # RuleManifest 型
├── diagnostic.rs       # Diagnostic/Severity/Fix 型
├── error.rs            # PluginError 型
└── test_utils.rs       # テストユーティリティ（test-utils feature）
```
