# tsuzulint_core

リンターエンジンの中核クレート。ファイル探索、解析、ルール実行、キャッシングを統合します。

## 概要

`tsuzulint_core` は TsuzuLint プロジェクトの中核となる **リンターエンジン** です。textlint にインスパイアされた高パフォーマンスな自然言語リンターとして、以下の主要な責務を担います：

- **リンターの調整**: ファイル探索、解析、ルール実行、キャッシングの統合
- **設定管理**: JSON/JSONC 形式の設定ファイルの読み込みと検証
- **並列処理 (rayon)**: 複数ファイルの高速並列リント実行
- **キャッシング**: BLAKE3 ハッシュに基づくインクリメンタルキャッシュ
- **WASM プラグイン統合**: tsuzulint_plugin を通じたルール実行

## アーキテクチャ

### データフロー

```text
CLI (パターン受信)
    ↓
Linter::lint_patterns()
    ↓
discover_files() → glob/walkdir でファイル探索
    ↓
lint_files_parallel() [rayon 並列処理]
    ↓ (各ファイル)
┌─────────────────────────────────────────┐
│ 1. キャッシュチェック (content hash +    │
│    config hash + rule versions)          │
│    - ヒット → キャッシュから結果を返す    │
│    - ミス → 解析実行                     │
│                                          │
│ 2. パーサー選択 (拡張子に基づく)          │
│    - .md/.markdown → MarkdownParser      │
│    - その他 → PlainTextParser            │
│                                          │
│ 3. AST 構築 (AstArena 使用)              │
│                                          │
│ 4. トークン化 & 文分割                   │
│                                          │
│ 5. インクリメンタルブロックキャッシュ    │
│    - 変更されていないブロックは再利用    │
│                                          │
│ 6. WASM ルール実行                       │
│    - Global isolation: ドキュメント全体  │
│    - Block isolation: 変更ブロックのみ   │
│                                          │
│ 7. 結果のキャッシュ保存                  │
└─────────────────────────────────────────┘
    ↓
結果集約 → 出力
```

### モジュール構成

| モジュール | 責務 |
| ---------- | ---- |
| `linter.rs` | メインの `Linter` 構造体。リンターの調整役 |
| `parallel_linter.rs` | rayon を使用した並列ファイルリント処理 |
| `file_linter.rs` | 単一ファイルのリントロジック |
| `config.rs` | 設定ファイルの読み込み、検証 (JSON Schema) |
| `rule_loader.rs` | プラグイン/ルールのロードと PluginHost 初期化 |
| `pool.rs` | PluginHost のスレッドプール |
| `walker.rs` | `ignore` クレートを使用した並列ファイル探索 |
| `context.rs` | LintContext - ドキュメント構造のキャッシュ |
| `fix.rs` / `fixer.rs` | 自動修正機能 |
| `block_extractor.rs` | インクリメンタルキャッシュ用のブロック抽出 |
| `manifest_resolver.rs` | ルールマニフェストのパス解決 |
| `formatters/sarif.rs` | SARIF 2.1.0 形式の出力 |

## 設定処理

### LinterConfig 構造体

```rust
pub struct LinterConfig {
    pub rules: Vec<RuleDefinition>,           // ロードするプラグイン
    pub options: HashMap<String, RuleOption>, // ルール設定
    pub include: Vec<String>,                 // 含めるファイルパターン
    pub exclude: Vec<String>,                 // 除外するファイルパターン
    pub cache: CacheConfig,                   // キャッシュ設定
    pub timings: bool,                        // パフォーマンス計測
    pub base_dir: Option<PathBuf>,            // 設定ファイルの基準ディレクトリ
}
```

### 設定ファイル形式

- **対応形式**: `.tsuzulint.json`, `.tsuzulint.jsonc` (コメント対応)
- **JSON Schema 検証**: 埋め込みスキーマによるバリデーション

### ルール定義パターン

```json
{
  "rules": [
    "owner/repo",
    { "github": "owner/repo@v1.0", "as": "alias" },
    { "url": "https://...", "as": "url-rule" },
    { "path": "./local/rule.json", "as": "local" }
  ],
  "options": {
    "no-todo": true,
    "max-lines": { "max": 100 },
    "disabled-rule": false
  }
}
```

## 並列処理

### rayon による並列リント

```rust
let results: Vec<Result<LintResult, (PathBuf, LinterError)>> = paths
    .par_iter()
    .map_init(
        || create_plugin_host(config, dynamic_rules),
        |host_result, path| {
            lint_file_internal(path, file_host, ...)
        }
    )
    .collect();
```

**特徴:**

- **map_init**: 各スレッドで PluginHost を初期化（WASM 再ロード回避）
- **スレッドセーフ**: Mutex で保護されたキャッシュアクセス
- **エラー分離**: 成功と失敗を分けて返す

## パフォーマンス最適化

### 1. インクリメンタルブロックキャッシュ

- ファイルをブロック単位でキャッシュ
- 変更されていないブロックの診断結果を再利用
- O(Blocks + Diagnostics) の効率的な分散

### 2. PluginHost プーリング

```rust
pub struct PluginHostPool {
    available: Mutex<VecDeque<PluginHost>>,
    initializer: Option<Arc<HostInitializer>>,
}
```

- WASM モジュールをロードしたままホストを再利用
- LIFO 順序で CPU キャッシュ効率を最大化

### 3. 早期ルールフィルタリング

```rust
pub struct ContentCharacteristics {
    pub has_headings: bool,
    pub has_links: bool,
    pub has_code_blocks: bool,
}

pub fn should_skip_rule(&self, node_types: &[String]) -> bool
```

ドキュメント内容を事前分析し、不要なルールをスキップ。

### 4. RawValue によるシリアライズ最適化

- 単一ルール時は直接 AST を渡す
- 複数ルール時は `RawValue` で一度だけシリアライズ

## 自動修正機能

```rust
use tsuzulint_core::apply_fixes_to_file;

for result in &successes {
    if !result.diagnostics.is_empty() {
        apply_fixes_to_file(&result.path, &result.diagnostics)?;
    }
}
```

- 依存グラフによる修正順序の決定
- トポロジカルソートによる安全な適用
- 反復適用による修正の連鎖対応

## 使用例

```rust
use tsuzulint_core::{Linter, LinterConfig};

// 設定ファイルから読み込み
let config = LinterConfig::from_file(".tsuzulint.json")?;
let linter = Linter::new(config)?;

// パターンでリント
let results = linter.lint_patterns(&["src/**/*.md".to_string()])?;
let (successes, failures) = results?;

for result in successes {
    println!("{}: {} issues", result.path.display(), result.diagnostics.len());
}

// SARIF 出力
use tsuzulint_core::generate_sarif;
let sarif = generate_sarif(&successes)?;
println!("{}", sarif);
```

## Feature Flags

```toml
[features]
default = ["native"]
native = ["tsuzulint_plugin/native"]    # Extism (ネイティブ WASM)
browser = ["tsuzulint_plugin/browser"]   # wasmi (ブラウザ WASM)
```

## 依存関係

| 依存関係 | 用途 |
| -------- | ---- |
| `rayon` | データ並列処理 |
| `blake3` | 高速なコンテンツハッシュ計算 |
| `serde` / `serde_json` | JSON シリアライズ |
| `jsonc-parser` | JSONC 解析 |
| `jsonschema` | JSON Schema 検証 |
| `walkdir` / `ignore` | ファイル探索 |
| `globset` | glob パターンマッチング |
| `crossbeam-channel` | マルチプロデューサー/コンシューマーチャネル |
| `parking_lot` | 高性能 Mutex |

## 公開 API

```rust
pub use config::{CacheConfig, LinterConfig, RuleDefinition};
pub use context::{DocumentStructure, LintContext};
pub use error::LinterError;
pub use fix::FixCoordinator;
pub use fixer::apply_fixes_to_file;
pub use formatters::generate_sarif;
pub use linter::Linter;
pub use pool::PluginHostPool;
pub use result::LintResult;
pub use tsuzulint_plugin::{Diagnostic, Fix, Severity};
```

## セキュリティ

### パストラバーサル対策

- マニフェストパス: 絶対パス、`..` コンポーネントを拒否
- WASM パス: マニフェストディレクトリ外へのパスを拒否
- canonicalize 検証: シンボリックリンク攻撃対策
