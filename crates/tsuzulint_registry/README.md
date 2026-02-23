# tsuzulint_registry

プラグインレジストリとパッケージ管理を担当するクレート。GitHub、URL、ローカルパスからプラグインを取得・キャッシュします。

## 概要

`tsuzulint_registry` は、TsuzuLint プロジェクトにおける **プラグインレジストリおよびパッケージ管理** を担当します。

### 主な役割

1. **外部ルールプラグインの取得**: GitHub、URL、ローカルパスからプラグインマニフェストを取得
2. **WASM アーティファクトのダウンロード**: プラグインの WASM バイナリを安全にダウンロード
3. **キャッシュ管理**: ダウンロードしたプラグインをローカルにキャッシュし、再利用を効率化
4. **セキュリティ保護**: SSRF 攻撃防止のための URL 検証、ハッシュ検証

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────────────────┐
│                      PluginResolver                              │
│  (プラグイン解決の中心的な調整役)                                   │
└─────────────────────────────────────────────────────────────────┘
          │                    │                    │
          ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ ManifestFetcher │  │  WasmDownloader │  │   PluginCache   │
│ (マニフェスト取得) │  │ (WASMダウンロード) │  │   (キャッシュ管理) │
└─────────────────┘  └─────────────────┘  └─────────────────┘
          │                    │                    │
          ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  PluginSource   │  │ HashVerifier    │  │  ファイルシステム  │
│ (取得元の種類)    │  │ (SHA256検証)     │  │  ~/.cache/...   │
└─────────────────┘  └─────────────────┘  └─────────────────┘
          │                    │
          ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                    validate_url (セキュリティ)                    │
│           SSRF 防止: ループバック/プライベートIP検証               │
└─────────────────────────────────────────────────────────────────┘
```

## PluginSource（プラグイン取得元）

3 種類のソースをサポート:

```rust
pub enum PluginSource {
    /// GitHub リポジトリ: `owner/repo` または `owner/repo@version`
    GitHub { owner: String, repo: String, version: Option<String> },
    /// 直接 URL
    Url(String),
    /// ローカルファイルパス
    Path(PathBuf),
}
```

### GitHub ソース

- `owner/repo` → 最新リリースから取得
- `owner/repo@v1.2.3` → 特定バージョンから取得
- URL 形式: `{base}/{owner}/{repo}/releases/download/v{version}/tsuzulint-rule.json`

## 解決フロー

```text
PluginSpec 解析
    ↓
キャッシュ確認
    ├─ ヒット → ResolvedPlugin 返却
    └─ ミス → 取得実行
           ↓
       マニフェスト取得
           ↓
       WASM ダウンロード
           ↓
       SHA256 ハッシュ検証
           ↓
       キャッシュ保存
           ↓
       ResolvedPlugin 返却
```

### PluginSpec パース形式

```json
// 文字列形式
"owner/repo"
"owner/repo@v1.0.0"

// オブジェクト形式
{"github": "owner/repo", "as": "my-rule"}
{"url": "https://example.com/manifest.json", "as": "external-rule"}
{"path": "./local/rule", "as": "local-rule"}
```

## キャッシュ管理

### キャッシュ場所

`~/.cache/tsuzulint/plugins/`（Unix 系）

### ディレクトリ構造

```text
~/.cache/tsuzulint/plugins/
├── owner/
│   └── repo/
│       └── v1.0.0/
│           ├── rule.wasm
│           └── tsuzulint-rule.json
└── url/
    └── {sha256_of_url}/
        └── v1.0.0/
            ├── rule.wasm
            └── tsuzulint-rule.json
```

### 機能

- パストラバーサル攻撃防止
- キャッシュされたマニフェストの `artifacts.wasm` をローカルパスに書き換え
- URL ソースは URL の SHA256 ハッシュをキーとして使用

## セキュリティ機能

### SSRF 対策 (`validate_url`)

**デフォルトでブロック:**

- `localhost` ドメイン
- IPv4 ループバック (`127.0.0.0/8`)
- IPv4 未指定 (`0.0.0.0`)
- IPv4 プライベート (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`)
- IPv4 リンクローカル (`169.254.0.0/16`)
- IPv6 ループバック (`::1`)
- IPv6 未指定 (`::`)
- IPv6 ユニークローカル (`fc00::/7`)
- IPv6 リンクローカル (`fe80::/10`)
- 非 HTTP スキーム（`ftp://`, `file://` 等）

**テスト/開発用に許可:**

```rust
let fetcher = ManifestFetcher::new().allow_local(true);
let downloader = WasmDownloader::new().allow_local(true);
```

### ハッシュ検証

- ダウンロードした WASM の SHA256 を自動計算
- マニフェストの `artifacts.sha256` と照合
- 不一致時は `HashError::Mismatch` を返却

### パストラバーサル対策

**ローカルパスソース:**

- 絶対パス禁止
- `..` コンポーネント禁止
- 正規化後のパスがマニフェストの親ディレクトリ内にあることを確認

**キャッシュ:**

- `owner`, `repo`, `version` が単一の正常なパスコンポーネントであることを検証

## WasmDownloader

```rust
pub struct WasmDownloader {
    max_size: usize,          // デフォルト: 50MB
    timeout: Duration,        // デフォルト: 60秒
    allow_local: bool,        // デフォルト: false
}
```

**機能:**

- ストリーミングダウンロード（大容量ファイル対応）
- サイズ制限チェック（事前・ストリーミング中の二段階）
- タイムアウト設定
- `{version}` プレースホルダー置換
- 自動ハッシュ計算

## 使用例

### 基本的な使用方法

```rust
use tsuzulint_registry::{PluginResolver, PluginSpec};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // リゾルバを作成
    let resolver = PluginResolver::new()?;
    
    // GitHub からプラグインを解決
    let spec = PluginSpec::parse(&json!("simorgh3196/tsuzulint-rule-no-todo@v1.0.0"))?;
    let resolved = resolver.resolve(&spec).await?;
    
    println!("WASM path: {:?}", resolved.wasm_path);
    println!("Manifest path: {:?}", resolved.manifest_path);
    println!("Alias: {}", resolved.alias);
    
    Ok(())
}
```

### カスタム設定

```rust
use tsuzulint_registry::downloader::WasmDownloader;
use std::time::Duration;

let downloader = WasmDownloader::with_options(
    100 * 1024 * 1024,        // 100MB max size
    Duration::from_secs(120), // 2 min timeout
);
```

### CLI からの使用

```bash
# GitHub からインストール
tzlint plugin install owner/repo

# 特定バージョン
tzlint plugin install owner/repo@v1.0.0

# エイリアス付き
tzlint plugin install owner/repo --as my-rule

# URL から
tzlint plugin install --url https://example.com/rule.wasm --as external-rule

# キャッシュクリア
tzlint plugin cache clean
```

## モジュール構成

| モジュール | 責務 |
| ---------- | ---- |
| `lib.rs` | エントリーポイント、公開 API の再エクスポート |
| `fetcher.rs` | プラグインマニフェストの取得 |
| `downloader.rs` | WASM バイナリのダウンロード |
| `resolver.rs` | プラグイン解決の統合 |
| `cache.rs` | プラグインのローカルキャッシュ |
| `security.rs` | URL セキュリティ検証 |
| `hash.rs` | SHA256 ハッシュ計算・検証 |
| `error.rs` | エラー型の定義 |

## 依存関係

| クレート | 用途 |
| --------- | ---- |
| `tsuzulint_manifest` | プラグインマニフェストの型定義 |
| `reqwest` | HTTP クライアント（ストリーミング対応） |
| `sha2` | SHA256 ハッシュ計算 |
| `hex` | ハッシュ値の16進数エンコード |
| `futures-util` | 非同期ストリーミング処理 |
| `dirs` | キャッシュディレクトリ取得 |
| `url` | URL パース |
| `tokio` | 非同期ランタイム |
| `tracing` | ログ出力 |
