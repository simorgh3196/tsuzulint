# tsuzulint_wasm

ブラウザ向け WebAssembly バインディングを提供するクレート。Rust で実装されたリンターをブラウザ環境で動作可能にします。

## 概要

**tsuzulint_wasm** は、TsuzuLint のブラウザ向け WebAssembly バインディングを提供するクレートです。Rust で実装された高機能なテキストリンターをブラウザ環境で動作可能にします。

**プロジェクト内の位置づけ:**

- TsuzuLint アーキテクチャの最上位層に位置
- CLI 版（ネイティブ）とは別に、ブラウザ/Node.js 環境向けの独立したビルドを提供
- npm パッケージとして公開可能な構造を持つ

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────────────┐
│                      TextLinter (WASM)                      │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  wasm-bindgen エクスポート                           │    │
│  │  - new()                                            │    │
│  │  - loadRule(wasm_bytes)                             │    │
│  │  - configureRule(name, config)                      │    │
│  │  - lint(content, file_type)                         │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   tsuzulint_plugin (browser)                │
│                   WasmiExecutor (WASM-in-WASM)              │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   tsuzulint_parser / tsuzulint_text         │
└─────────────────────────────────────────────────────────────┘
```

## ビルド

### ビルドターゲット

`build.sh` は 3 種類のターゲットをサポート:

- `web`: ブラウザ直接使用（ES Modules）
- `nodejs`: Node.js 環境
- `bundler`: Webpack/Vite 等のバンドラー用

```bash
# ブラウザ向け
./build.sh web

# Node.js 向け
./build.sh nodejs

# バンドラー向け
./build.sh bundler
```

### クレートタイプ

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

- `cdylib`: WebAssembly バイナリとしてエクスポート
- `rlib`: Rust テスト用に通常のライブラリとしてもビルド可能

## API

### コンストラクタ

```javascript
const linter = new TextLinter();
```

### メソッド

| Rust メソッド | JS 名 | 説明 |
| -------------- | ------ | ------ |
| `load_rule(&mut self, wasm_bytes: &[u8])` | `loadRule` | WASM ルールをバイト列からロード |
| `configure_rule(&mut self, name: &str, config_json: JsValue)` | `configureRule` | ルールの設定 |
| `loaded_rules(&self)` | `getLoadedRules` | ロード済みルール一覧取得 |
| `lint(&mut self, content: &str, file_type: &str)` | `lint` | リント実行（JS オブジェクト返却） |
| `lint_json(&mut self, content: &str, file_type: &str)` | `lintJson` | リント実行（JSON 文字列返却） |

## 使用例

### ブラウザ

```html
<!DOCTYPE html>
<html>
<head>
  <script type="module">
    import init, { TextLinter } from './pkg/tsuzulint_wasm.js';

    async function main() {
      await init();

      const linter = new TextLinter();

      // ルールをロード（WASM バイト列を fetch などで取得）
      const ruleResponse = await fetch('./rules/no-todo.wasm');
      const ruleBytes = await ruleResponse.arrayBuffer();
      linter.loadRule(new Uint8Array(ruleBytes));

      // リント実行
      const content = '# Hello\n\nThis is a TODO item.';
      const diagnostics = linter.lint(content, 'markdown');

      console.log(diagnostics);
      // [
      //   {
      //     ruleId: "no-todo",
      //     message: "Found TODO keyword",
      //     start: 0,
      //     end: 4,
      //     severity: "warning",
      //     fix: { start: 0, end: 4, text: "DONE" }
      //   }
      // ]
    }

    main();
  </script>
</head>
<body>
  <textarea id="editor"></textarea>
  <div id="diagnostics"></div>
</body>
</html>
```

### Node.js

```javascript
const { TextLinter } = require('./pkg/tsuzulint_wasm.js');
const fs = require('fs');

const linter = new TextLinter();

// ローカルファイルからルールをロード
const ruleBytes = fs.readFileSync('./rules/no-todo.wasm');
linter.loadRule(ruleBytes);

// リント実行
const content = '# Hello\n\nThis is a TODO item.';
const diagnostics = linter.lint(content, 'markdown');

console.log(JSON.stringify(diagnostics, null, 2));
```

## 診断結果の形式

### JsDiagnostic

```typescript
interface JsDiagnostic {
  ruleId: string;
  message: string;
  start: number;
  end: number;
  startLine?: number;
  startColumn?: number;
  endLine?: number;
  endColumn?: number;
  severity: "error" | "warning" | "info";
  fix?: {
    start: number;
    end: number;
    text: string;
  };
}
```

## 内部パイプライン

```text
1. 入力
   ├── 引数: content: &str, file_type: &str
   
2. パーサー選択
   ├── "markdown" | "md" → MarkdownParser
   └── その他 → PlainTextParser
   
3. 解析
   ├── AstArena で AST 構築
   ├── JSON に変換
   └── prepare_text_analysis():
       ├── Tokenizer でトークン化
       └── SentenceSplitter で文分割
   
4. ルール実行
   └── host.run_all_rules_with_parts()
       └── 各 WASM ルールを wasmi で実行
   
5. 出力変換
   ├── Diagnostic → JsDiagnostic
   └── serde_wasm_bindgen::to_value()
```

## WASM-in-WASM 設計

ブラウザ環境では、`tsuzulint_plugin` の `browser` フィーチャーを使用し、wasmi（純粋 Rust WASM インタープリタ）でルールを実行:

- ネイティブ版の Extism/wasmtime はブラウザ環境で動作しない
- wasmi は自身を WASM にコンパイル可能 → **WASM-in-WASM** 実行が可能

## 依存関係

| 依存クレート | 目的 |
| ------------ | ------ |
| `tsuzulint_ast` | AST データ構造 |
| `tsuzulint_parser` | Markdown/PlainText パーサー |
| `tsuzulint_plugin` (`browser` feature) | WASM ルール実行エンジン |
| `tsuzulint_text` | トークナイザー・センテンス分割 |
| `wasm-bindgen` | Rust ↔ JavaScript 間の FFI バインディング |
| `serde-wasm-bindgen` | Serde 型を JsValue に変換 |
| `serde / serde_json` | シリアライゼーション |
| `js-sys` | JavaScript 標準オブジェクトへのアクセス |
| `console_error_panic_hook` | panic 時にコンソールにスタックトレース出力 |

## npm パッケージ構成

```json
{
  "name": "tsuzulint-wasm",
  "files": [
    "tsuzulint_wasm_bg.wasm",
    "tsuzulint_wasm_bg.wasm.d.ts",
    "tsuzulint_wasm.js",
    "tsuzulint_wasm.d.ts"
  ]
}
```

TypeScript 型定義（`.d.ts`）が自動生成され、型安全な利用が可能。

## テスト

```rust
#[wasm_bindgen_test]
fn test_lint_basic() {
    let mut linter = TextLinter::new().unwrap();
    // ... テストロジック
}
```

- `wasm-bindgen-test`: WASM 環境でのテスト実行
- `build.rs`: テスト用の simple_rule WASM フィクスチャを自動ビルド

## 制限事項

- wasmi はインタプリタのため、ネイティブ版より低速
- 大きなファイルの処理には時間がかかる可能性がある
