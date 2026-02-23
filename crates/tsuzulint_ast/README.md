# tsuzulint_ast

TxtAST (textlint AST) の型定義と Arena アロケータを提供するクレート。

## 概要

`tsuzulint_ast` は、TsuzuLint プロジェクトの中核をなす **AST（抽象構文木）型定義**を提供します。textlint の TxtAST 仕様との互換性を保ちながら、Rust のメモリモデルに最適化された実装を提供します。

## 主な機能

- **TxtAST 型定義**: 自然言語テキストの AST 表現
- **Arena アロケーション**: bumpalo を使用した高速なメモリ管理
- **Visitor パターン**: AST 走査と変換のためのトレイト
- **JSON シリアライゼーション**: WASM ルールとのデータ受け渡し

## アーキテクチャ

### Arena Allocation (bumpalo)

Oxc にインスパイアされたアリーナアロケーションを採用:

```rust
pub struct AstArena {
    bump: Bump,  // bumpalo のラッパー
}
```

**利点:**

1. **アロケーションオーバーヘッドの最小化** - バンプアロケーションは非常に高速
2. **キャッシュ局所性の向上** - 1ファイルの全ノードが連続メモリに配置
3. **一括解放** - `reset()` で全メモリを一気に解放
4. **ゼロコピー文字列** - `alloc_str()` でソーステキストを直接参照

### 主要な型

#### `TxtNode<'a>` - コア AST ノード型

```rust
pub struct TxtNode<'a> {
    pub node_type: NodeType,          // ノード種別
    pub span: Span,                   // ソーステキスト内のバイト位置
    pub children: &'a [TxtNode<'a>],  // 子ノード（アリーナ参照）
    pub value: Option<&'a str>,       // テキスト値（Str, Code, CodeBlock 用）
    pub data: NodeData<'a>,           // 追加データ
}
```

- ライフタイム `'a` によりアリーナアロケータに紐付け
- `Copy` トレイトを実装（コピーが安価）
- 3種類のコンストラクタ: `new_parent()`, `new_text()`, `new_leaf()`

#### `NodeType` - ノード種別の列挙型

```rust
pub enum NodeType {
    // ドキュメント構造
    Document, Paragraph, Header, BlockQuote, List, ListItem,
    CodeBlock, HorizontalRule, Html,
    // インライン要素
    Str, Break, Emphasis, Strong, Delete, Code, Link, Image,
    // 参照要素 (textlint v14.5.0+)
    LinkReference, ImageReference, Definition,
    // GFM 拡張
    Table, TableRow, TableCell,
    FootnoteDefinition, FootnoteReference,
}
```

#### `NodeData<'a>` - ノード固有データ

従来の構造体アプローチと比較して **30%以上のメモリ削減**を実現:

```rust
pub enum NodeData<'a> {
    None,                                    // データなし
    Header(u8),                              // 見出しレベル (1-6)
    List(bool),                              // ordered フラグ
    CodeBlock(Option<&'a str>),              // 言語指定
    Link(LinkData<'a>),                      // URL + タイトル
    Image(LinkData<'a>),
    Reference(ReferenceData<'a>),            // 識別子 + ラベル
    Definition(DefinitionData<'a>),
}
```

### Visitor パターン

#### `Visitor<'a>` - 読み取り専用走査

```rust
pub trait Visitor<'a>: Sized {
    fn enter_node(&mut self, node: &TxtNode<'a>) -> VisitResult;
    fn exit_node(&mut self, node: &TxtNode<'a>) -> VisitResult;
    fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult;
    // ... 各ノードタイプ用の visit_* メソッド
}
```

**制御フロー:**
- `ControlFlow::Continue(())` - 子ノードの走査を継続
- `ControlFlow::Break(())` - 早期終了（`?` 演算子で伝播）

#### `MutVisitor<'a>` - AST 変換

```rust
pub trait MutVisitor<'a>: Sized {
    fn arena(&self) -> &'a AstArena;  // 新ノード割り当て用
    fn visit_str_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a>;
    // ... 他すべてのノードタイプ
}
```

## 使用例

```rust
use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span, NodeData};

// アリーナの作成
let arena = AstArena::new();

// テキストノードの作成
let text = arena.alloc(TxtNode::new_text(
    NodeType::Str,
    Span::new(0, 5),
    arena.alloc_str("hello"),
));

// 子ノードスライスの作成
let children = arena.alloc_slice_copy(&[*text]);

// 親ノードの作成
let paragraph = arena.alloc(TxtNode::new_parent(
    NodeType::Paragraph,
    Span::new(0, 5),
    children,
    NodeData::None,
));
```

### Visitor の使用例

```rust
use tsuzulint_ast::{Visitor, TxtNode, walk_node};
use std::ops::ControlFlow;

struct TextCollector<'a> {
    texts: Vec<&'a str>,
}

impl<'a> Visitor<'a> for TextCollector<'a> {
    fn visit_str(&mut self, node: &TxtNode<'a>) -> ControlFlow<()> {
        if let Some(text) = node.value {
            self.texts.push(text);
        }
        ControlFlow::Continue(())
    }
}
```

## 依存関係

| 依存関係 | 用途 |
| ------- | ------ |
| **bumpalo** | アリーナアロケーション |
| **serde** | シリアライゼーション（WASM ルールへの JSON 受け渡し） |
| **rkyv** (optional) | ゼロコピーシリアライゼーション（キャッシュ用） |

## textlint TxtAST との互換性

TxtAST 仕様 に準拠:

- `type` フィールド: ノード種別（PascalCase）
- `range` フィールド: `[start, end]` バイトオフセット
- `children` フィールド: 子ノード配列（親ノードのみ）
- `value` フィールド: テキスト値（テキストノードのみ）
- 追加フィールド: `depth`, `ordered`, `lang`, `url`, `title`, `identifier`, `label`

## モジュール構成

```text
src/
├── lib.rs           # 公開API、クレートドキュメント
├── arena.rs         # AstArena（bumpaloラッパー）
├── node.rs          # TxtNode, NodeData, LinkData等
├── node_type.rs     # NodeType列挙型
├── span.rs          # Span, Position, Location
└── visitor/
    ├── mod.rs       # Visitorモジュール定義
    ├── visit.rs     # Visitor trait（読み取り）
    ├── visit_mut.rs # MutVisitor trait（変換）
    └── walk.rs      # walk_node, walk_children関数
```
