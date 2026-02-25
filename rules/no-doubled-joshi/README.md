# no-doubled-joshi

一文の中で同じ助詞が重複している場合にエラーを報告するルールです。

このルールは [textlint-rule-no-doubled-joshi](https://github.com/textlint-ja/textlint-rule-no-doubled-joshi) と互換性があります。

## 概要

日本語の文章において、同じ助詞が近接して繰り返されると文が読みにくくなります。このルールはそのような重複を検出し、改善を促します。

### エラーになる例

```markdown
私は彼は好きだ。
```

「は」が近接して2回使用されているためエラーになります。

### エラーにならない例

```markdown
既存のコードの利用を推奨する。
```

「の」（連体化）は例外扱いされるためエラーになりません。

## 設定オプション

| オプション | 型 | デフォルト | 説明 |
| :--- | :--- | :--- | :--- |
| `min_interval` | number | `1` | 同じ助詞間の最小間隔（1 = 読点を挟んで隣接） |
| `strict` | boolean | `false` | 例外ルールを無効化 |
| `allow` | string[] | `[]` | 重複を許可する助詞のリスト |
| `separator_characters` | string[] | `[".", "．", "。", "?", "!", "？", "！"]` | 文区切り文字 |
| `comma_characters` | string[] | `["、", "，"]` | 間隔を増やす読点文字 |
| `suggest_fix` | boolean | `false` | 自動修正候補を提供するかどうか |

### min_interval

同じ助詞間の最小間隔を設定します。

- `1`（デフォルト）: 読点（、）や括弧を1つ挟めば許可
- `2` 以上: さらに離れている必要がある

```json
{
  "rules": {
    "no-doubled-joshi": {
      "min_interval": 2
    }
  }
}
```

### strict

例外ルールを無効化します。デフォルトでは以下の助詞は重複が許可されます：

- `の`（連体化）
- `を`（格助詞）
- `て`（接続助詞）
- 並立助詞の連続
- `かどうか`パターン

```json
{
  "rules": {
    "no-doubled-joshi": {
      "strict": true
    }
  }
}
```

### allow

特定の助詞の重複を常に許可します。

```json
{
  "rules": {
    "no-doubled-joshi": {
      "allow": ["も", "や"]
    }
  }
}
```

## 例外ルール

デフォルト設定では、以下のケースはエラーになりません：

### `の`（連体化）

```markdown
既存のコードの利用を推奨する。
```

### `を`（格助詞）

```markdown
オブジェクトを返す関数を公開する。
```

### `て`（接続助詞）

```markdown
まずは試していただきて、ご意見をお聞かせください。
```

### 並立助詞

```markdown
登ったり降りたりする。
```

### `かどうか`パターン

```markdown
これにするかどうか検討する。
```

### 異なる細分類

同じ表記でも品詞の細分類が異なる場合は別の助詞として扱われます：

```markdown
ターミナルで「test」と入力すると画面が表示される。
```

- `と`（格助詞）と`と`（接続助詞）は別扱い

### 連語

連続する助詞は1つの連語として扱われます：

```markdown
文字列には長さがある。
```

`に` + `は` = `には`（連語）

## エラーメッセージ

```text
一文に二回以上利用されている助詞 "は" がみつかりました。

次の助詞が連続しているため、文を読みにくくしています。

- 私"は"
- 彼"は"

同じ助詞を連続して利用しない、文の中で順番を入れ替える、文を分割するなどを検討してください。
```

## 参考資料

- [textlint-rule-no-doubled-joshi](https://github.com/textlint-ja/textlint-rule-no-doubled-joshi) - このルールのオリジナル実装
- [textlint-ja/textlint-rule-no-doubled-joshi テストケース](https://github.com/textlint-ja/textlint-rule-no-doubled-joshi/blob/master/test/no-doubled-joshi-test.ts)

## ライセンス

MIT
