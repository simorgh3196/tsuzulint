# tsuzulint_cache

ファイルレベルのキャッシングシステムを提供するクレート。BLAKE3 ハッシュによるインクリメンタルキャッシュを実装します。

## 概要

`tsuzulint_cache` は、TsuzuLint プロジェクトにおける **ファイルレベルのキャッシングシステム** を提供します。

### 主な役割

- **変更されていないファイルの再 lint 回避**: コンテンツハッシュを比較し、変更がないファイルの解析をスキップ
- **設定変更時のキャッシュ無効化**: ルール設定が変更された場合、関連キャッシュを自動的に無効化
- **ルールバージョン追跡**: WASM ルールのバージョンが変更された場合、キャッシュを無効化
- **インクリメンタル更新**: ブロック単位での差分キャッシングをサポート

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────────────┐
│                      CacheManager                            │
├─────────────────────────────────────────────────────────────┤
│  entries: HashMap<String, CacheEntry>                       │
│  cache_dir: PathBuf                                          │
│  enabled: bool                                               │
├─────────────────────────────────────────────────────────────┤
│  ┌───────────────┐    ┌──────────────────┐                  │
│  │  load/save    │◄──►│  cache.rkyv      │  (Disk)         │
│  │  (rkyv)       │    │  Zero-Copy       │                  │
│  └───────────────┘    └──────────────────┘                  │
├─────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────┐  │
│  │                    CacheEntry                          │  │
│  │  - content_hash: String (BLAKE3)                      │  │
│  │  - config_hash: String                                │  │
│  │  - rule_versions: HashMap<String, String>             │  │
│  │  - diagnostics: Vec<Diagnostic>                       │  │
│  │  - blocks: Vec<BlockCacheEntry>                       │  │
│  │    └─ hash: [u8; 32] (BLAKE3)                        │  │
│  │    └─ span: Span                                      │  │
│  │    └─ diagnostics: Vec<Diagnostic>                   │  │
│  │  - created_at: u64                                    │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## キャッシュキー生成 (BLAKE3)

### 実装

```rust
pub fn hash_content(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}
```

### BLAKE3 採用理由

- **高速性**: SHA-256 よりも高速（SIMD 最適化対応）
- **セキュリティ**: 暗号学的に安全なハッシュ関数
- **一貫性**: 同一入力から常に同一の 256 ビット（64文字の16進数）ハッシュを生成

### キャッシュキー構成要素

1. **コンテンツハッシュ**: ファイル内容の BLAKE3 ハッシュ
2. **設定ハッシュ**: lint 設定のハッシュ
3. **ルールバージョン**: `HashMap<String, String>` 形式でルール名とバージョンを管理
4. **ブロックハッシュ**: 32 バイト配列 (`[u8; 32]`) でブロック単位の差分検出

## キャッシュ無効化戦略

### 検証ロジック

```rust
pub fn is_valid(
    &self,
    content_hash: &str,
    config_hash: &str,
    rule_versions: &HashMap<String, String>,
) -> bool {
    self.content_hash == content_hash
        && self.config_hash == config_hash
        && self.rule_versions == *rule_versions
}
```

### 無効化トリガー

| 条件 | 結果 |
| ------ | ------ |
| ファイル内容が変更 | キャッシュ無効化 |
| 設定ファイルが変更 | キャッシュ無効化 |
| ルールのバージョンが変更 | キャッシュ無効化 |
| ルールの数が変更 | キャッシュ無効化 |
| ルール名が変更 | キャッシュ無効化 |

## インクリメンタルブロックキャッシュ

`reconcile_blocks()` メソッドにより、ブロック単位での差分再利用を実現:

```rust
pub fn reconcile_blocks(
    &self,
    path: &Path,
    current_blocks: &[BlockCacheEntry],
    config_hash: &str,
    rule_versions: &HashMap<String, String>,
) -> (Vec<Diagnostic>, Vec<bool>)
```

**動作原理:**

1. キャッシュされたブロックをハッシュ値でマッピング
2. 現在のブロックとハッシュ値が一致する場合、診断結果を再利用
3. **位置シフト補正**: ブロックが移動した場合、診断の Span を補正
4. `find_best_match()` で最も近い位置の候補を選択（位置シフト最小化）

## キャッシュストレージ形式

### Zero-Copy Deserialization (rkyv)

```rust
// 保存
let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&self.entries)?;
fs::write(&cache_file, bytes)?;

// 読み込み
let content = fs::read(&cache_file)?;
let entries: HashMap<String, CacheEntry> =
    rkyv::from_bytes::<_, rkyv::rancor::Error>(&content)?;
```

### ファイル構造

```text
<cache_dir>/
└── cache.rkyv    # rkyv 形式のバイナリファイル
```

### rkyv の利点

| 特徴 | 説明 |
| ------ | ------ |
| **パース不要** | バイト列から直接アクセス |
| **メモリ効率** | 追加割り当てなしでデータアクセス |
| **高速起動** | キャッシュロード時のオーバーヘッド最小化 |
| **アーカイブ型** | `rkyv::Archive` derive で自動生成 |

## 使用例

### 基本的な使用方法

```rust
use tsuzulint_cache::{CacheManager, CacheEntry};
use std::collections::HashMap;

// キャッシュマネージャーの作成
let mut manager = CacheManager::new(".cache/tsuzulint")?;

// コンテンツハッシュの計算
let content_hash = CacheManager::hash_content("source text");

// キャッシュの確認
if let Some(entry) = manager.get("path/to/file.md") {
    if entry.is_valid(&content_hash, &config_hash, &rule_versions) {
        // キャッシュヒット - 診断結果を再利用
        return Ok(entry.diagnostics);
    }
}

// キャッシュミス - lint 実行後、結果をキャッシュ
let entry = CacheEntry {
    content_hash,
    config_hash,
    rule_versions,
    diagnostics,
    blocks: vec![],
    created_at: timestamp(),
};
manager.set("path/to/file.md", entry);

// ディスクに保存
manager.save()?;
```

### インクリメンタルブロックキャッシュの使用例

```rust
// ブロック単位でキャッシュを活用
let (cached_diagnostics, block_validity) = manager.reconcile_blocks(
    &path,
    &current_blocks,
    &config_hash,
    &rule_versions,
);

// 変更されたブロックのみ再 lint
for (i, is_valid) in block_validity.iter().enumerate() {
    if !is_valid {
        // このブロックを再 lint
    }
}
```

## 公開 API

```rust
pub use entry::CacheEntry;
pub use error::CacheError;
pub use manager::CacheManager;
```

### CacheManager 主なメソッド

| メソッド | 説明 |
| ---------- | ------ |
| `new(cache_dir)` | キャッシュマネージャー作成 |
| `enable()` / `disable()` | キャッシュの有効/無効切り替え |
| `get(path)` | キャッシュエントリ取得 |
| `set(path, entry)` | キャッシュエントリ保存 |
| `is_valid(...)` | キャッシュ有効性チェック |
| `reconcile_blocks(...)` | ブロック単位の差分再利用 |
| `load()` | ディスクからキャッシュ読み込み |
| `save()` | ディスクへキャッシュ保存 |
| `clear()` | 全キャッシュクリア |
| `hash_content(content)` | コンテンツの BLAKE3 ハッシュ生成 |

## Feature Flags

```toml
[features]
default = ["native"]
native = ["tsuzulint_plugin/native"]
browser = ["tsuzulint_plugin/browser"]
```

- **native**: ネイティブ環境向け（CLI 使用）
- **browser**: ブラウザ WASM 環境向け（tsuzulint_wasm 使用）

## 依存関係

| クレート | 用途 |
| ---------- | ------ |
| **blake3** | 高速な暗号学的ハッシュ関数。コンテンツ/ブロックのハッシュ生成 |
| **rkyv** | Zero-copy シリアライゼーション。キャッシュの永続化 |
| **serde / serde_json** | JSON シリアライゼーション |
| **thiserror** | エラー型定義 |
| **tracing** | ログ出力 |
| **tsuzulint_plugin** | `Diagnostic` 型の共有 |
| **tsuzulint_ast** | `Span` 型の共有 |
