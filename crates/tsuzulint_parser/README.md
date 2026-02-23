# tsuzulint_parser

パーサー抽象レイヤーを提供するクレート。Markdown、プレーンテキストなどのフォーマットを TxtAST に変換します。

## 概要

`tsuzulint_parser` は、TsuzuLint プロジェクトにおける **パーサー抽象レイヤー** を提供します。このクレートの役割は以下の通りです：

1. **テキスト形式の抽象構文木（TxtAST）への変換**: ソーステキストを解析し、統一された AST 形式に変換
2. **カスタムパーサーの実装基盤**: `Parser` トレイトを通じて、新しいファイル形式のサポートを容易にする
3. **フォーマット依存部分の分離**: コアの linter ロジックからパーサー実装を切り離し、拡張性を確保

## アーキテクチャ

```text
┌─────────────────────────────────────────────────────────────┐
│                     tsuzulint_parser                        │
│  ┌─────────────────┐  ┌─────────────────┐                   │
│  │   Parser trait  │  │   ParseError    │                   │
│  │  - name()       │  │  - InvalidSource│                   │
│  │  - extensions() │  │  - Unsupported  │                   │
│  │  - parse()      │  │  - Internal     │                   │
│  │  - can_parse()  │  └─────────────────┘                   │
│  └────────┬────────┘                                        │
│           │ implements                                      │
│  ┌────────┴────────┐                                        │
│  │                 │                                        │
│  ▼                 ▼                                        │
│ ┌───────────────┐ ┌───────────────┐                         │
│ │MarkdownParser │ │PlainTextParser│                         │
│ │               │ │               │                         │
│ │ - markdown-rs │ │ - 空行区切り  │                         │
│ │ - GFM対応     │ │ - 段落ベース  │                         │
│ └───────┬───────┘ └───────┬───────┘                         │
└─────────┼─────────────────┼─────────────────────────────────┘
          │                 │
          ▼                 ▼
┌─────────────────────────────────────────────────────────────┐
│                      tsuzulint_ast                          │
│  ┌───────────┐  ┌──────────┐  ┌───────────┐  ┌───────────┐ │
│  │ AstArena  │  │ TxtNode  │  │ NodeType  │  │ NodeData  │ │
│  │ (bumpalo) │  │          │  │           │  │           │ │
│  └───────────┘  └──────────┘  └───────────┘  └───────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Parser トレイト

```rust
pub trait Parser {
    /// パーサーの名前を返す（例: "markdown", "text"）
    fn name(&self) -> &str;

    /// サポートするファイル拡張子を返す（ドットなし、例: ["md", "markdown"]）
    fn extensions(&self) -> &[&str];

    /// ソーステキストを TxtAST に変換
    fn parse<'a>(&self, arena: &'a AstArena, source: &str) -> Result<TxtNode<'a>, ParseError>;

    /// 指定された拡張子を処理できるか判定（デフォルト実装、大文字小文字を区別しない）
    fn can_parse(&self, extension: &str) -> bool;
}
```

**設計ポイント:**

- `parse` メソッドは Arena アロケータ（`AstArena`）を引数に取り、そのアリーナ上に AST ノードを割り当てる
- これにより、パース完了後のメモリ解放が O(1) で行える
- `can_parse` はデフォルト実装を提供し、大文字小文字を区別しないマッチングを行う

## 組み込みパーサー

### MarkdownParser

**使用ライブラリ**: `markdown-rs` (wooorm/markdown-rs)

**サポートする拡張子**: `["md", "markdown", "mdown", "mkdn", "mkd"]`

**パースオプション**: GFM (GitHub Flavored Markdown) デフォルト

**サポートする mdast ノードと TxtAST NodeType のマッピング:**

| mdast Node | TxtAST NodeType | 説明 |
| ---------- | --------------- | ---- |
| Root | Document | ドキュメントルート |
| Paragraph | Paragraph | 段落 |
| Heading | Header | 見出し（深度情報付き） |
| Text | Str | テキスト |
| Emphasis | Emphasis | 斜体 |
| Strong | Strong | 太字 |
| InlineCode | Code | インラインコード |
| Code | CodeBlock | コードブロック（言語情報付き） |
| Link | Link | リンク（URL/タイトル付き） |
| Image | Image | 画像（URL/タイトル付き） |
| List | List | リスト（順序/非順序） |
| ListItem | ListItem | リスト項目 |
| Blockquote | BlockQuote | 引用ブロック |
| ThematicBreak | HorizontalRule | 水平線 |
| Break | Break | 改行 |
| Html | Html | HTML 要素 |
| Delete | Delete | 取り消し線（GFM） |
| Table | Table | テーブル（GFM） |
| TableRow | TableRow | テーブル行 |
| TableCell | TableCell | テーブルセル |
| FootnoteDefinition | FootnoteDefinition | 脚注定義（GFM） |
| FootnoteReference | FootnoteReference | 脚注参照（GFM） |
| LinkReference | LinkReference | リンク参照 |
| ImageReference | ImageReference | 画像参照 |
| Definition | Definition | 定義 |

### PlainTextParser

**サポートする拡張子**: `["txt", "text"]`

**パースロジック:**

- テキストを空行で区切って段落に分割
- 各段落は `Paragraph` ノードとして表現
- 段落内のテキストは `Str` ノードとして表現

**出力 AST 構造:**

```text
Document
└── Paragraph (各段落)
    └── Str (テキスト)
```

## 使用例

### 基本的な使用方法

```rust
use tsuzulint_parser::{MarkdownParser, Parser};
use tsuzulint_ast::AstArena;

let parser = MarkdownParser::new();
let arena = AstArena::new();

let source = "# Hello\n\nThis is **bold** text.";
let ast = parser.parse(&arena, source)?;

// AST の走査
for child in ast.children {
    println!("Node type: {:?}", child.node_type);
}
```

### 拡張子によるパーサー選択

```rust
use tsuzulint_parser::{MarkdownParser, PlainTextParser, Parser};

fn get_parser(extension: &str) -> Box<dyn Parser> {
    let md = MarkdownParser::new();
    let txt = PlainTextParser::new();
    
    if md.can_parse(extension) {
        Box::new(md)
    } else {
        Box::new(txt)
    }
}
```

### カスタムパーサーの実装

```rust
use tsuzulint_parser::{Parser, ParseError};
use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span, NodeData};

struct YamlParser;

impl Parser for YamlParser {
    fn name(&self) -> &str {
        "yaml"
    }

    fn extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }

    fn parse<'a>(
        &self,
        arena: &'a AstArena,
        source: &str,
    ) -> Result<TxtNode<'a>, ParseError> {
        // YAML パースロジックを実装
        // ...
    }
}
```

## 依存関係

| クレート | 用途 |
| ------- | ---- |
| `tsuzulint_ast` | AST 型定義、Arena アロケータ、NodeType など |
| `markdown` | Markdown のパース（mdast 互換の出力） |
| `thiserror` | エラー型の定義（derive macro） |

## エラー処理

```rust
pub enum ParseError {
    /// ソーステキストが無効
    InvalidSource { message: String, offset: Option<usize> },
    /// サポートされていない機能
    Unsupported(String),
    /// 内部パーサーエラー
    Internal(String),
}
```

- `thiserror` クレートを使用したエラー定義
- エラー発生位置（バイトオフセット）を含む詳細なエラー情報を提供

## モジュール構成

```text
src/
├── lib.rs        # 公開API、クレートドキュメント
├── traits.rs     # Parser トレイト定義
├── error.rs      # ParseError 型
├── markdown.rs   # MarkdownParser 実装
└── text.rs       # PlainTextParser 实装
```
