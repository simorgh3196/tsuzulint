# tsuzulint_manifest

外部ルールのマニフェストファイル（`tsuzulint-rule.json`）を定義・検証するための共有ライブラリ。

## 概要

`tsuzulint_manifest` は、TsuzuLint プロジェクトにおいて **外部ルールのマニフェストファイル** を定義・検証するための共有ライブラリです。

**主な役割:**

- 外部 WASM ルールの配布メタデータを表現する型定義の提供
- JSON Schema に基づくマニフェスト検証機能の提供
- 他のクレート（`tsuzulint_registry`, `tsuzulint_plugin` など）で共通利用される型の集約

このクレートは、ルールの配布・インストール時に使用され、ルールの名前、バージョン、WASM ファイルの場所、整合性チェック用ハッシュ、権限設定などの情報を一元管理します。

## マニフェスト構造

### ExternalRuleManifest（ルート構造体）

| フィールド | 型 | 必須 | 説明 |
| ---------- | -- | ---- | ---- |
| `rule` | `RuleMetadata` | Yes | ルールのメタデータ |
| `artifacts` | `Artifacts` | Yes | ダウンロード可能な成果物情報 |
| `permissions` | `Option<Permissions>` | No | 必要な権限 |
| `tsuzulint` | `Option<TsuzuLintCompatibility>` | No | TsuzuLint 互換性情報 |
| `options` | `Option<Value>` | No | ルール設定オプションの JSON Schema |

### RuleMetadata（ルールメタデータ）

| フィールド | 型 | 必須 | 説明 |
| ---------- | - | ---- | ---- |
| `name` | `String` | Yes | ルール識別子。パターン: `^[a-z][a-z0-9-]*$` |
| `version` | `String` | Yes | セマンティックバージョン |
| `description` | `Option<String>` | No | ルールの説明 |
| `repository` | `Option<String>` | No | GitHub リポジトリ URL |
| `license` | `Option<String>` | No | ライセンス識別子（SPDX 形式推奨） |
| `authors` | `Vec<String>` | No | 作者リスト |
| `keywords` | `Vec<String>` | No | 検索用キーワード |
| `fixable` | `bool` | No | 自動修正が可能かどうか（デフォルト: `false`） |
| `node_types` | `Vec<String>` | No | 処理対象の AST ノードタイプ（空 = 全ノード） |
| `isolation_level` | `IsolationLevel` | No | ルールの分離レベル（デフォルト: `Global`） |

### IsolationLevel

```rust
pub enum IsolationLevel {
    Global,  // ドキュメント全体が必要
    Block,   // 個別ブロック単位で実行可能
}
```

### Artifacts（成果物情報）

| フィールド | 型 | 必須 | 説明 |
| ---------- | - | ---- | ---- |
| `wasm` | `String` | Yes | WASM ファイルのダウンロード URL。`{version}` プレースホルダー使用可能 |
| `sha256` | `String` | Yes | WASM ファイルの SHA256 ハッシュ（64文字の16進数） |

### Permissions（権限設定）

| フィールド | 型 | 説明 |
| ---------- | - | ---- |
| `filesystem` | `Vec<FilesystemPermission>` | ファイルシステムアクセス権限 |
| `network` | `Vec<NetworkPermission>` | ネットワークアクセス権限 |

## マニフェストファイルの例

```json
{
  "rule": {
    "name": "no-todo",
    "version": "1.0.0",
    "description": "Disallow TODO comments in text",
    "repository": "https://github.com/owner/tsuzulint-rule-no-todo",
    "license": "MIT",
    "authors": ["Author Name"],
    "keywords": ["todo", "comments"],
    "fixable": true,
    "node_types": ["Str"],
    "isolation_level": "Block"
  },
  "artifacts": {
    "wasm": "https://github.com/owner/tsuzulint-rule-no-todo/releases/download/v{version}/rule.wasm",
    "sha256": "abc123def456...64chars"
  },
  "permissions": {
    "filesystem": [],
    "network": []
  },
  "tsuzulint": {
    "min_version": "0.1.0"
  },
  "options": {
    "type": "object",
    "properties": {
      "allow": {
        "type": "array",
        "items": { "type": "string" },
        "description": "List of allowed TODO patterns"
      }
    }
  }
}
```

## JSON Schema 検証

### 埋め込みスキーマ

```rust
const RULE_SCHEMA_JSON: &str = include_str!("../../../schemas/v1/rule.json");
```

- JSON Schema ファイルを **コンパイル時に埋め込み**
- `include_str!` マクロにより、実行時のファイル I/O が不要
- バイナリにスキーマが含まれるため、デプロイが簡素化

### 遅延初期化パターン

```rust
static SCHEMA: OnceLock<Validator> = OnceLock::new();
```

- `OnceLock` を使用して、バリデーターを **スレッドセーフに一度だけ初期化**
- 最初の検証時にのみスキーマをコンパイル
- 以降の検証はコンパイル済みバリデーターを再利用

### 検証フロー

```rust
pub fn validate_manifest(json_str: &str) -> Result<ExternalRuleManifest, ManifestError>
```

1. **JSON パース**: 入力文字列を `serde_json::Value` に変換
2. **スキーマ検証**: JSON Schema Draft-07 に準拠して検証
3. **構造体デシリアライズ**: 検証済み JSON を `ExternalRuleManifest` に変換

## エラーハンドリング

```rust
pub enum ManifestError {
    ParseError(#[from] serde_json::Error),  // JSON パースエラー
    ValidationError(String),                 // スキーマ検証エラー
}
```

- `thiserror` を使用したエラー定義
- ユーザーフレンドリーなエラーメッセージ（エラー箇所のパスを含む）

## 使用例

### 基本的な使用方法

```rust
use tsuzulint_manifest::{validate_manifest, ExternalRuleManifest};

let json = r#"{
    "rule": {
        "name": "no-todo",
        "version": "1.0.0",
        "description": "Disallow TODO comments"
    },
    "artifacts": {
        "wasm": "https://example.com/rule.wasm",
        "sha256": "abc123...64chars"
    }
}"#;

match validate_manifest(json) {
    Ok(manifest) => {
        println!("Rule: {} v{}", manifest.rule.name, manifest.rule.version);
    }
    Err(e) => {
        eprintln!("Validation failed: {}", e);
    }
}
```

### 構造体から直接作成

```rust
use tsuzulint_manifest::{ExternalRuleManifest, RuleMetadata, Artifacts};

let manifest = ExternalRuleManifest {
    rule: RuleMetadata {
        name: "no-todo".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Disallow TODO comments".to_string()),
        ..Default::default()
    },
    artifacts: Artifacts {
        wasm: "https://example.com/rule.wasm".to_string(),
        sha256: "abc123...".to_string(),
    },
    ..Default::default()
};

// JSON に変換
let json = serde_json::to_string_pretty(&manifest)?;
```

## 依存関係

| 依存クレート | 用途 |
| ------------ | ---- |
| **`jsonschema`** | JSON Schema Draft-07 準拠のバリデーション |
| **`serde`** | シリアライズ/デシリアライズフレームワーク |
| **`serde_json`** | JSON パースと操作 |
| **`thiserror`** | エラー型定義 |

## 設計上の特徴

1. **単一責任**: マニフェストの型定義と検証のみに集中
2. **スキーマ中心**: JSON Schema を唯一の真実のソースとして扱う
3. **ゼロコピー設計**: `OnceLock` によるスキーマ再利用
4. **拡張性**: `options` フィールドで任意の JSON Schema を許容

## 他クレートとの連携

- **`tsuzulint_registry`**: プラグインダウンロード時にマニフェストを検証
- **`tsuzulint_plugin`**: ルールロード時にマニフェスト情報を使用
- **`tsuzulint_core`**: ルール設定のバリデーションに使用
