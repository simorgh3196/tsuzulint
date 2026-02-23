# tsuzulint_lsp

Language Server Protocol (LSP) サーバー実装。エディタ/IDE でのリアルタイムリンティング機能を提供します。

## 概要

**tsuzulint_lsp** は TsuzuLint の Language Server Protocol (LSP) サーバー実装です。エディタ/IDE でのリアルタイムリンティング機能を提供し、以下の役割を担います：

- エディタとの標準化された通信
- ドキュメントのリアルタイム検証と診断の提供
- 自動修正機能の提供
- ドキュメントシンボル（アウトライン）の提供
- 設定ファイルの動的リロード

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────┐
│                    Backend                          │
│  (tower-lsp LanguageServer trait 実装)              │
├─────────────────────────────────────────────────────┤
│  BackendState                                       │
│  ├── documents: RwLock<HashMap<Url, DocumentData>>  │
│  ├── linter: RwLock<Option<Linter>>                │
│  └── workspace_root: RwLock<Option<PathBuf>>       │
└─────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────┐
│              tsuzulint_core::Linter                 │
│              tsuzulint_parser::Parser               │
└─────────────────────────────────────────────────────┘
```

## 実装されている LSP 機能

### テキスト同期

| メソッド | 機能 | 説明 |
| ------- | ------ | ------ |
| `textDocument/didOpen` | ドキュメントオープン | ドキュメントをキャッシュに保存し、即座に検証実行 |
| `textDocument/didChange` | ドキュメント変更 | 変更内容をキャッシュに反映、**300ms デバウンス**後に検証 |
| `textDocument/didSave` | ドキュメント保存 | 保存時に再検証 |
| `textDocument/didClose` | ドキュメントクローズ | キャッシュから削除、診断をクリア |

### 診断

- **リアルタイム診断**: ドキュメントオープン/変更時に自動的にリントを実行
- **非ブロッキング実行**: `tokio::task::spawn_blocking` でリント処理をオフロード
- **デバウンス**: `didChange` 時に 300ms の遅延で連続入力時の過剰なリントを防止

### コードアクション（自動修正）

| 種類 | タイトル | 説明 |
| ------ | --------- | ------ |
| Quick Fix | "Fix: {診断メッセージ}" | 個別の診断に対する単一修正 |
| Source Fix All | "Fix all TsuzuLint issues" | 一括で全ての修正可能な問題を修正 |

### ドキュメントシンボル

Markdown ドキュメントから構造を抽出:

- Header → `SymbolKind::STRING`
- CodeBlock → `SymbolKind::FUNCTION`

エディタのアウトラインビューで表示可能。

### ファイル監視

`workspace/didChangeWatchedFiles` で設定ファイルの変更を監視し、自動的にリロード。

## ServerCapabilities

```rust
ServerCapabilities {
    text_document_sync: TextDocumentSyncOptions {
        open_close: true,
        change: TextDocumentSyncKind::FULL,
        save: SaveOptions { include_text: true }
    },
    code_action_provider: CodeActionOptions {
        code_action_kinds: [QUICKFIX, SOURCE_FIX_ALL]
    },
    document_symbol_provider: true
}
```

## 使用方法

### CLI から起動

```bash
tzlint lsp
```

### Neovim での設定例

```lua
local lspconfig = require('lspconfig')

lspconfig.tsuzulint.setup {
  cmd = { "tzlint", "lsp" },
  filetypes = { "markdown" },
  root_dir = lspconfig.util.root_pattern('.tsuzulint.json', '.tsuzulint.jsonc', '.git'),
  settings = {},
}
```

### VS Code 拡張機能

拡張機能の `package.json`:

```json
{
  "contributes": {
    "languages": [
      {
        "id": "markdown",
        "extensions": [".md", ".markdown"]
      }
    ],
    "grammars": []
  },
  "activationEvents": [
    "onLanguage:markdown"
  ]
}
```

拡張機能の TypeScript:

```typescript
import * as vscode from 'vscode';

export function activate(context: vscode.ExtensionContext) {
  const serverOptions: vscode.ServerOptions = {
    command: 'tzlint',
    args: ['lsp']
  };

  const clientOptions: vscode.LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'markdown' }],
  };

  const client = new vscode.LanguageClient(
    'tsuzulint',
    'TsuzuLint',
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(client.start());
}
```

## デバウンス動作

`textDocument/didChange` では、連続した入力時にリント処理が過剰に実行されないよう、300ms のデバウンスを適用:

1. ドキュメント変更イベントを受信
2. 300ms 待機
3. 待機中に新しい変更がない場合 → リント実行
4. 待機中に新しい変更があった場合 → 待機をリセット

## エラーハンドリング

- **RwLock poisoning**: ロック取得失敗時にログ出力して早期リターン
- **Linter 初期化失敗**: `None` で格納し、リントなしで継続動作
- **lint_text 失敗**: 空のベクタを返し、エラーログ出力

## tower-lsp の使用

```rust
pub async fn run() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
```

## 依存関係

| 依存関係 | 用途 |
| ------- | ------ |
| `tsuzulint_core` | リンター本体 |
| `tsuzulint_parser` | パーサー |
| `tsuzulint_ast` | AST 型 |
| `tower-lsp` | LSP フレームワーク |
| `tokio` | 非同期ランタイム |
| `serde_json` | JSON シリアライズ |
| `tracing` | ログ出力 |

## 現状と制限事項

- **基本的な実装**: 現在は基本的な LSP 機能のみ実装
- **診断キャッシュなし**: `codeAction` で再リントを実行（最適化の余地あり）
- **フラットなシンボルリスト**: ネストしたシンボル構造は未サポート
- **単純なデバウンス**: 固定 300ms 遅延

## テスト

- **ユニットテスト**: バイトオフセット → LSP Position 変換
- **統合テスト**: デバウンス動作の検証
