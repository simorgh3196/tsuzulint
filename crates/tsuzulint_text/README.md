# tsuzulint_text

テキスト分析コンポーネント。形態素解析（トークン化）と文分割機能を提供します。

## 概要

`tsuzulint_text` は **TsuzuLint プロジェクトのテキスト分析コンポーネント** です。自然言語リンターにおいて重要な以下の 2 つの中核機能を提供します：

- **形態素解析**: 日本語テキストのトークン化
- **文分割**: UAX #29 準拠 + 日本語特有のヒューリスティクスによる文境界検出

これらはリンティングルール（例：「文末の助詞をチェック」「1 文の長さ制限」など）の基盤となる機能です。

## モジュール構成

```text
tsuzulint_text/
├── Cargo.toml
├── README.md
├── examples/
│   └── uax29_test.rs      # UAX #29 動作確認用の例
└── src/
    ├── lib.rs             # 公開API定義
    ├── tokenizer.rs       # 形態素解析器
    └── splitter.rs        # 文分割器
```

## Tokenizer（形態素解析）

Lindera（MeCab ベースの Rust 形態素解析ライブラリ）を使用した日本語テキストのトークン化を行います。

### Token 構造体

```rust
pub struct Token {
    pub surface: String,       // 表層形（テキストそのもの）
    pub pos: Vec<String>,      // 品詞（例: ["名詞", "一般"]）
    pub detail: Vec<String>,   // 詳細品詞情報
    pub span: Range<usize>,    // 元テキスト内のバイト範囲
}
```

### Tokenizer

```rust
pub struct Tokenizer { /* ... */ }

impl Tokenizer {
    /// IPADIC 辞書（埋め込み版）を使用してインスタンス生成
    pub fn new() -> Result<Self, TextError>;
    
    /// テキストをトークン列に変換
    pub fn tokenize(&self, text: &str) -> Result<Vec<Token>, TextError>;
}
```

### 処理ロジック

1. IPADIC 辞書をロード（`embedded://ipadic`）
2. Lindera の `Normal` モードでセグメンテーション
3. 各トークンから以下を抽出:
   - `surface`: 表層形
   - `pos`: 品詞情報（最大 4 要素、`*` は除外）
   - `detail`: 詳細情報（5 要素目以降）
   - `span`: バイト位置範囲

### 使用例

```rust
use tsuzulint_text::Tokenizer;

let tokenizer = Tokenizer::new()?;
let tokens = tokenizer.tokenize("こんにちは世界");

for token in tokens {
    println!("{}: {:?}", token.surface, token.pos);
}
// 出力:
// こんにちは: ["感動詞"]
// 世界: ["名詞", "一般"]
```

## SentenceSplitter（文分割）

Unicode Standard Annex #29 (UAX #29) に基づく文分割に、**日本語特有のヒューリスティクス** を組み合わせたハイブリッドアプローチ。

### なぜハイブリッドか？

標準的な UAX #29 ルールは日本語テキストに対して過剰に分割する傾向があります:

- `すごい！！本当に！？` → UAX #29 では「！！」後で分割
- 日本語の強調表現では分割せず 1 文として扱いたい

### Sentence 構造体

```rust
pub struct Sentence {
    pub text: String,          // 文のテキスト内容
    pub span: Range<usize>,    // 元テキスト内のバイト範囲
}
```

### SentenceSplitter

```rust
impl SentenceSplitter {
    /// テキストを文に分割
    pub fn split(text: &str, ignore_ranges: &[Range<usize>]) -> Vec<Sentence>;
}
```

- `ignore_ranges`: インラインコードや URL など、分割禁止範囲を指定

### 分割ルール

| 条件 | 動作 |
| ---- | ---- |
| **日本語句点 `。`** | 常に分割 |
| **感嘆符・疑問符 (`！？!?`)** | 後続が空白/改行の場合 → 分割<br>後続が非空白の場合 → 分割抑制 |
| **単一改行 `\n`** | 分割抑制（ソフトラップ扱い） |
| **二重改行 `\n\n`** | 常に分割（段落区切り扱い） |
| **無視範囲内** | 分割しない |

### 実装のポイント

1. `unicode_sentences()` で UAX #29 ベースの境界を取得
2. ポインタ演算でバイトオフセットを計算
3. `should_split()` で日本語ヒューリスティクスを適用
4. ギャップ（セグメント間の文字）は前の文にマージ

### 文分割の使用例

```rust
use tsuzulint_text::SentenceSplitter;

let text = "こんにちは。世界。";
let sentences = SentenceSplitter::split(text, &[]);

for sentence in sentences {
    println!("{}: {:?}", sentence.text, sentence.span);
}
// 出力:
// こんにちは。: 0..15
// 世界。: 15..24
```

### コードブロック保護

```rust
let text = "これは `code.` です。次の文。";
let ignore_ranges = vec![10..17]; // `code.` の範囲
let sentences = SentenceSplitter::split(text, &ignore_ranges);
// 結果: ["これは `code.` です。", "次の文。"]
```

## 日本語ヒューリスティクスの詳細

### 感嘆符・疑問符の処理

```text
入力: "すごい！！本当に！？"

UAX #29 のみ:
  ["すごい！！", "本当に！？"]  // 過剰分割

日本語ヒューリスティクス適用:
  ["すごい！！本当に！？"]     // 適切にマージ
```

### 空白後の分割

```text
入力: "すごい！！ 本当に！？"

結果:
  ["すごい！！", "本当に！？"]  // 空白後で分割
```

## プロジェクト全体での位置づけ

```text
tsuzulint_core
    └── tsuzulint_parser     # パーサー（Markdown/PlainText）
            └── tsuzulint_text  ← このクレート
                    ├── トークン化（ルール用）
                    └── 文分割（文単位ルール用）
```

テキスト分析結果は:

- **トークン**: 「助詞の重複」「品詞パターンチェック」などのルールで使用
- **文**: 「1 文の最大長」「文の数え上げ」などのルールで使用

## 依存関係

| 依存クレート | 用途 |
| ------------ | ---- |
| **lindera** | 日本語形態素解析。`embed-ipadic` feature で IPADIC 辞書を埋め込み |
| **unicode-segmentation** | UAX #29 準拠の文境界検出 |
| **serde** | `Token`, `Sentence` のシリアライズ/デシリアライズ |
| **thiserror** | カスタムエラー型定義 |
| **miette** | ユーザーフレンドリーなエラー表示 |

### なぜこれらの依存関係か？

- **lindera**: MeCab 互換の高精度な日本語形態素解析を Rust で実装。IPADIC 埋め込みで外部辞書不要
- **unicode-segmentation**: UAX #29 準拠のセグメンテーション。`unicode_sentences()` が文境界検出を提供
- **serde**: AST ノードとして WASM ルールに渡す際に JSON シリアライズが必要

## エラー型

```rust
pub enum TextError {
    TokenizeError(String),    // トークン化エラー
}
```

## 公開 API

```rust
pub use splitter::{Sentence, SentenceSplitter};
pub use tokenizer::{TextError, Token, Tokenizer};
```
