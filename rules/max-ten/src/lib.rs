//! max-ten rule: Limit the number of Japanese commas (読点) in a single sentence.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | max | number | 3 | Maximum number of commas allowed in a sentence |
//! | strict | boolean | false | If false, commas sandwiched between nouns are ignored |
//! | touten | string | "、" | The comma character to count |
//! | kuten | string | "。" | The period character |

use extism_pdk::*;
use serde::Deserialize;
use tsuzulint_rule_pdk::{
    Capability, Diagnostic, KnownLanguage, LintRequest, LintResponse, RuleManifest, Span, Token,
    is_node_type,
};

const RULE_ID: &str = "max-ten";
const VERSION: &str = "1.0.0";

const DEFAULT_MAX_TEN: usize = 3;
const DEFAULT_TOUTEN: &str = "、";
const DEFAULT_KUTEN: &str = "。";

#[derive(Debug, serde::Serialize, Deserialize)]
struct Config {
    #[serde(default = "default_max")]
    max: usize,
    #[serde(default)]
    strict: bool,
    #[serde(default = "default_touten")]
    touten: String,
    #[serde(default = "default_kuten")]
    kuten: String,
}

fn default_max() -> usize {
    DEFAULT_MAX_TEN
}

fn default_touten() -> String {
    DEFAULT_TOUTEN.to_string()
}

fn default_kuten() -> String {
    DEFAULT_KUTEN.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max: DEFAULT_MAX_TEN,
            strict: false,
            touten: DEFAULT_TOUTEN.to_string(),
            kuten: DEFAULT_KUTEN.to_string(),
        }
    }
}

#[plugin_fn]
pub fn get_manifest() -> FnResult<RuleManifest> {
    Ok(RuleManifest::new(RULE_ID, VERSION)
        .with_description("Limit the number of Japanese commas (読点) in a single sentence")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()])
        .with_languages(vec![KnownLanguage::Ja])
        .with_capabilities(vec![Capability::Morphology]))
}

#[plugin_fn]
pub fn lint(request: LintRequest) -> FnResult<LintResponse> {
    do_lint(request)
}

fn do_lint(request: LintRequest) -> FnResult<LintResponse> {
    let mut diagnostics = Vec::new();

    if !is_node_type(&request.node, "Str") {
        return Ok(LintResponse { diagnostics });
    }

    let config: Config = request.get_config().unwrap_or_default();
    let tokens = request.get_tokens();

    let mut separator_characters = vec!["?", "!", "？", "！"];
    separator_characters.push(&config.kuten);

    let mut current_ten_count = 0;
    let mut last_touten_span = None;

    for (index, token) in tokens.iter().enumerate() {
        let surface = token.surface.as_str();

        if surface == config.touten {
            let is_sandwiched = is_sandwiched_meishi(tokens, index);
            if !config.strict && is_sandwiched {
                continue;
            }
            current_ten_count += 1;
            last_touten_span = Some(token.span);
        }

        if separator_characters.contains(&surface) {
            current_ten_count = 0;
        }

        if current_ten_count > config.max {
            if let Some(span) = last_touten_span {
                diagnostics.push(Diagnostic::new(
                    RULE_ID,
                    format!(
                        "一つの文で\"{}\"を{}つ以上使用しています",
                        config.touten,
                        config.max + 1
                    ),
                    Span::new(span.start, span.end),
                ));
            }
            current_ten_count = 0;
        }
    }

    Ok(LintResponse { diagnostics })
}

fn is_sandwiched_meishi(tokens: &[Token], index: usize) -> bool {
    let before = find_sibling_meaning_token(tokens, index, -1);
    let after = find_sibling_meaning_token(tokens, index, 1);

    if let (Some(b), Some(a)) = (before, after) {
        b.is_noun() && a.is_noun()
    } else {
        false
    }
}

fn find_sibling_meaning_token(
    tokens: &[Token],
    current_index: usize,
    direction: isize,
) -> Option<&Token> {
    let mut idx = current_index as isize + direction;
    while idx >= 0 && (idx as usize) < tokens.len() {
        let sibling = &tokens[idx as usize];
        if is_kakko(sibling) {
            idx += direction;
            continue;
        }
        return Some(sibling);
    }
    None
}

fn is_kakko(token: &Token) -> bool {
    if token.major_pos() == Some("記号") {
        if let Some(detail) = token.pos_detail(1) {
            if detail.starts_with("括弧") {
                return true;
            }
        }
    }
    if token.surface == "(" || token.surface == ")" {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tsuzulint_rule_pdk::{AstNode, TextSpan};

    fn create_token(surface: &str, pos: Vec<&str>, start: u32, end: u32) -> Token {
        Token::new(
            surface,
            pos.iter().map(|s| s.to_string()).collect(),
            TextSpan::new(start, end),
        )
    }

    fn create_request_with_config<T: serde::Serialize>(
        text: &str,
        config: &T,
        tokens: Vec<Token>,
    ) -> LintRequest {
        let mut request = LintRequest::single(
            AstNode::new("Str", Some([0, text.len() as u32])),
            text.to_string(),
        );
        request.config = Some(rmp_serde::to_vec_named(config).unwrap());
        // Configure text context mock with tokens for helpers
        request.helpers = Some(tsuzulint_rule_pdk::LintHelpers {
            text_context: Some(tsuzulint_rule_pdk::TextContext {
                tokens,
                sentences: vec![],
            }),
            ..Default::default()
        });
        request
    }

    struct TestCase {
        text: String,
        config: Option<serde_json::Value>,
        tokens: Vec<Token>,
        expected_errors: Vec<ExpectedError>,
    }

    struct ExpectedError {
        message: String,
    }

    fn run_test_cases(valid: Vec<TestCase>, invalid: Vec<TestCase>) {
        for (i, case) in valid.into_iter().enumerate() {
            let request = if let Some(config) = case.config {
                create_request_with_config(&case.text, &config, case.tokens)
            } else {
                create_request_with_config(&case.text, &Config::default(), case.tokens)
            };
            let response = do_lint(request).unwrap();
            assert_eq!(
                response.diagnostics.len(),
                0,
                "Expected valid case {} to have 0 errors, found {}",
                i,
                response.diagnostics.len()
            );
        }

        for (i, case) in invalid.into_iter().enumerate() {
            let request = if let Some(config) = case.config {
                create_request_with_config(&case.text, &config, case.tokens)
            } else {
                create_request_with_config(&case.text, &Config::default(), case.tokens)
            };
            let response = do_lint(request).unwrap();
            assert_eq!(
                response.diagnostics.len(),
                case.expected_errors.len(),
                "Expected invalid case {} to have {} errors, found {}",
                i,
                case.expected_errors.len(),
                response.diagnostics.len()
            );
            for (diag, expected) in response.diagnostics.iter().zip(case.expected_errors.iter()) {
                assert_eq!(
                    diag.message, expected.message,
                    "Invalid case {} message mismatch\nDiag span: {:?}",
                    i, diag.span
                );
            }
        }
    }

    #[test]
    fn test_max_ten() {
        let valid = vec![
            TestCase {
                text: "名詞、名詞、名詞、名詞、名詞の場合は例外".to_string(),
                config: None,
                tokens: vec![
                    create_token("名詞", vec!["名詞", "一般"], 0, 6),
                    create_token("、", vec!["記号", "読点"], 6, 9),
                    create_token("名詞", vec!["名詞", "一般"], 9, 15),
                    create_token("、", vec!["記号", "読点"], 15, 18),
                    create_token("名詞", vec!["名詞", "一般"], 18, 24),
                    create_token("、", vec!["記号", "読点"], 24, 27),
                    create_token("名詞", vec!["名詞", "一般"], 27, 33),
                    create_token("、", vec!["記号", "読点"], 33, 36),
                    create_token("名詞", vec!["名詞", "一般"], 36, 42),
                    create_token("の", vec!["助詞", "連体化"], 42, 45),
                    create_token("場合", vec!["名詞", "一般"], 45, 51),
                    create_token("は", vec!["助詞", "係助詞"], 51, 54),
                    create_token("例外", vec!["名詞", "一般"], 54, 60),
                ],
                expected_errors: vec![],
            },
            TestCase {
                text: "ビスケットの主な材料は(1)小麦粉、(2)牛乳、(3)ショートニング、(4)バター、(5)砂糖である。".to_string(),
                config: None,
                tokens: vec![
                    create_token("ビスケット", vec!["名詞", "一般"], 0, 15),
                    create_token("(", vec!["記号", "一般"], 33, 34),
                    create_token("1", vec!["名詞", "数"], 34, 35),
                    create_token(")", vec!["記号", "一般"], 35, 36),
                    create_token("小麦粉", vec!["名詞", "一般"], 36, 45),
                    create_token("、", vec!["記号", "読点"], 45, 48),
                    create_token("(", vec!["記号", "一般"], 48, 49),
                    create_token("2", vec!["名詞", "数"], 49, 50),
                    create_token(")", vec!["記号", "一般"], 50, 51),
                    create_token("牛乳", vec!["名詞", "一般"], 51, 57),
                    create_token("、", vec!["記号", "読点"], 57, 60),
                    create_token("(", vec!["記号", "一般"], 60, 61),
                    create_token("3", vec!["名詞", "数"], 61, 62),
                    create_token(")", vec!["記号", "一般"], 62, 63),
                    create_token("ショートニング", vec!["名詞", "一般"], 63, 84),
                    create_token("、", vec!["記号", "読点"], 84, 87),
                    create_token("(", vec!["記号", "一般"], 87, 88),
                    create_token("4", vec!["名詞", "数"], 88, 89),
                    create_token(")", vec!["記号", "一般"], 89, 90),
                    create_token("バター", vec!["名詞", "一般"], 90, 99),
                    create_token("、", vec!["記号", "読点"], 99, 102),
                    create_token("(", vec!["記号", "一般"], 102, 103),
                    create_token("5", vec!["名詞", "数"], 103, 104),
                    create_token(")", vec!["記号", "一般"], 104, 105),
                    create_token("砂糖", vec!["名詞", "一般"], 105, 111),
                ],
                expected_errors: vec![],
            },
            TestCase {
                text: "これは、これは、これは、これは、オプションでカウントされないのでOK".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "touten": "，",
                    "kuten": "．"
                })).unwrap(),
                tokens: vec![
                    create_token("これ", vec!["代名詞", "一般"], 0, 6),
                    create_token("は", vec!["助詞", "係助詞"], 6, 9),
                    create_token("、", vec!["記号", "読点"], 9, 12),
                    create_token("これ", vec!["代名詞", "一般"], 12, 18),
                    create_token("は", vec!["助詞", "係助詞"], 18, 21),
                    create_token("、", vec!["記号", "読点"], 21, 24),
                    create_token("これ", vec!["代名詞", "一般"], 24, 30),
                    create_token("は", vec!["助詞", "係助詞"], 30, 33),
                    create_token("、", vec!["記号", "読点"], 33, 36),
                    create_token("これ", vec!["代名詞", "一般"], 36, 42),
                    create_token("は", vec!["助詞", "係助詞"], 42, 45),
                    create_token("、", vec!["記号", "読点"], 45, 48),
                ],
                expected_errors: vec![],
            },
            TestCase {
                text: "テキスト１、テキスト２、テキスト３、テキスト４、テキスト５".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 5
                })).unwrap(),
                tokens: vec![
                    create_token("テキスト", vec!["名詞", "一般"], 0, 12),
                    create_token("１", vec!["名詞", "数"], 12, 15),
                    create_token("、", vec!["記号", "読点"], 15, 18),
                    create_token("テキスト", vec!["名詞", "一般"], 18, 30),
                    create_token("２", vec!["名詞", "数"], 30, 33),
                    create_token("、", vec!["記号", "読点"], 33, 36),
                    create_token("テキスト", vec!["名詞", "一般"], 36, 48),
                    create_token("３", vec!["名詞", "数"], 48, 51),
                    create_token("、", vec!["記号", "読点"], 51, 54),
                    create_token("テキスト", vec!["名詞", "一般"], 54, 66),
                    create_token("４", vec!["名詞", "数"], 66, 69),
                    create_token("、", vec!["記号", "読点"], 69, 72),
                    create_token("テキスト", vec!["名詞", "一般"], 72, 84),
                    create_token("５", vec!["名詞", "数"], 84, 87),
                ],
                expected_errors: vec![],
            },
            TestCase {
                text: "「紅ちゃんは、たたえなくなっていいって、思う？ もっと大事なものがあったら、スパイになれなくてもいいって、思う？」".to_string(),
                config: None,
                tokens: vec![
                    create_token("「", vec!["記号", "括弧開"], 0, 3),
                    create_token("紅", vec!["名詞", "固有名詞"], 3, 6),
                    create_token("ちゃん", vec!["名詞", "接尾"], 6, 15),
                    create_token("は", vec!["助詞", "係助詞"], 15, 18),
                    create_token("、", vec!["記号", "読点"], 18, 21),
                    create_token("たたえ", vec!["動詞", "自立"], 21, 30),
                    create_token("なく", vec!["助動詞"], 30, 36),
                    create_token("なっ", vec!["動詞", "非自立"], 36, 42),
                    create_token("て", vec!["助詞", "接続助詞"], 42, 45),
                    create_token("いい", vec!["形容詞", "自立"], 45, 51),
                    create_token("って", vec!["助詞", "格助詞"], 51, 57),
                    create_token("、", vec!["記号", "読点"], 57, 60),
                    create_token("思う", vec!["動詞", "自立"], 60, 66),
                    create_token("？", vec!["記号", "一般"], 66, 69),
                    create_token(" ", vec!["記号", "空白"], 69, 70),
                    create_token("もっと", vec!["副詞", "一般"], 70, 79),
                    create_token("大事", vec!["名詞", "形容動詞語幹"], 79, 85),
                    create_token("な", vec!["助動詞"], 85, 88),
                    create_token("もの", vec!["名詞", "非自立"], 88, 94),
                    create_token("が", vec!["助詞", "格助詞"], 94, 97),
                    create_token("あっ", vec!["動詞", "自立"], 97, 103),
                    create_token("たら", vec!["助動詞"], 103, 109),
                    create_token("、", vec!["記号", "読点"], 109, 112),
                    create_token("スパイ", vec!["名詞", "一般"], 112, 121),
                    create_token("に", vec!["助詞", "格助詞"], 121, 124),
                    create_token("なれ", vec!["動詞", "自立"], 124, 130),
                    create_token("なく", vec!["助動詞"], 130, 136),
                    create_token("て", vec!["助詞", "接続助詞"], 136, 139),
                    create_token("も", vec!["助詞", "係助詞"], 139, 142),
                    create_token("いい", vec!["形容詞", "自立"], 142, 148),
                    create_token("って", vec!["助詞", "格助詞"], 148, 154),
                    create_token("、", vec!["記号", "読点"], 154, 157),
                    create_token("思う", vec!["動詞", "自立"], 157, 163),
                    create_token("？", vec!["記号", "一般"], 163, 166),
                    create_token("」", vec!["記号", "括弧閉"], 166, 169),
                ],
                expected_errors: vec![],
            },
        ];
        let invalid = vec![
            TestCase {
                text: "これは、これは、これは、これは、これはだめ。".to_string(),
                config: None,
                tokens: vec![
                    create_token("これ", vec!["代名詞", "一般"], 0, 6),
                    create_token("は", vec!["助詞", "係助詞"], 6, 9),
                    create_token("、", vec!["記号", "読点"], 9, 12),
                    create_token("これ", vec!["代名詞", "一般"], 12, 18),
                    create_token("は", vec!["助詞", "係助詞"], 18, 21),
                    create_token("、", vec!["記号", "読点"], 21, 24),
                    create_token("これ", vec!["代名詞", "一般"], 24, 30),
                    create_token("は", vec!["助詞", "係助詞"], 30, 33),
                    create_token("、", vec!["記号", "読点"], 33, 36),
                    create_token("これ", vec!["代名詞", "一般"], 36, 42),
                    create_token("は", vec!["助詞", "係助詞"], 42, 45),
                    create_token("、", vec!["記号", "読点"], 45, 48),
                    create_token("これ", vec!["代名詞", "一般"], 48, 54),
                    create_token("は", vec!["助詞", "係助詞"], 54, 57),
                    create_token("だめ", vec!["名詞", "一般"], 57, 63),
                    create_token("。", vec!["記号", "句点"], 63, 66),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "これは，これは，これは，これは，これは。".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "touten": "，",
                    "kuten": "．"
                }))
                .unwrap(),
                tokens: vec![
                    create_token("これ", vec!["代名詞", "一般"], 0, 6),
                    create_token("は", vec!["助詞", "係助詞"], 6, 9),
                    create_token("，", vec!["記号", "読点"], 9, 12),
                    create_token("これ", vec!["代名詞", "一般"], 12, 18),
                    create_token("は", vec!["助詞", "係助詞"], 18, 21),
                    create_token("，", vec!["記号", "読点"], 21, 24),
                    create_token("これ", vec!["代名詞", "一般"], 24, 30),
                    create_token("は", vec!["助詞", "係助詞"], 30, 33),
                    create_token("，", vec!["記号", "読点"], 33, 36),
                    create_token("これ", vec!["代名詞", "一般"], 36, 42),
                    create_token("は", vec!["助詞", "係助詞"], 42, 45),
                    create_token("，", vec!["記号", "読点"], 45, 48),
                    create_token("これ", vec!["代名詞", "一般"], 48, 54),
                    create_token("は", vec!["助詞", "係助詞"], 54, 57),
                    create_token("。", vec!["記号", "句点"], 57, 60),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"，\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "テキスト１、テキスト２、テキスト３、テキスト４、テキスト５".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 3,
                    "strict": true
                }))
                .unwrap(),
                tokens: vec![
                    create_token("テキスト", vec!["名詞", "一般"], 0, 12),
                    create_token("１", vec!["名詞", "数"], 12, 15),
                    create_token("、", vec!["記号", "読点"], 15, 18),
                    create_token("テキスト", vec!["名詞", "一般"], 18, 30),
                    create_token("２", vec!["名詞", "数"], 30, 33),
                    create_token("、", vec!["記号", "読点"], 33, 36),
                    create_token("テキスト", vec!["名詞", "一般"], 36, 48),
                    create_token("３", vec!["名詞", "数"], 48, 51),
                    create_token("、", vec!["記号", "読点"], 51, 54),
                    create_token("テキスト", vec!["名詞", "一般"], 54, 66),
                    create_token("４", vec!["名詞", "数"], 66, 69),
                    create_token("、", vec!["記号", "読点"], 69, 72),
                    create_token("テキスト", vec!["名詞", "一般"], 72, 84),
                    create_token("５", vec!["名詞", "数"], 84, 87),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "これは，これは，これは。これは，これは，これは，どうですか?".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "touten": "，",
                    "kuten": "．"
                }))
                .unwrap(),
                tokens: vec![
                    create_token("これ", vec!["代名詞", "一般"], 0, 6),
                    create_token("は", vec!["助詞", "係助詞"], 6, 9),
                    create_token("，", vec!["記号", "読点"], 9, 12),
                    create_token("これ", vec!["代名詞", "一般"], 12, 18),
                    create_token("は", vec!["助詞", "係助詞"], 18, 21),
                    create_token("，", vec!["記号", "読点"], 21, 24),
                    create_token("これ", vec!["代名詞", "一般"], 24, 30),
                    create_token("は", vec!["助詞", "係助詞"], 30, 33),
                    create_token("。", vec!["記号", "句点"], 33, 36),
                    create_token("これ", vec!["代名詞", "一般"], 36, 42),
                    create_token("は", vec!["助詞", "係助詞"], 42, 45),
                    create_token("，", vec!["記号", "読点"], 45, 48),
                    create_token("これ", vec!["代名詞", "一般"], 48, 54),
                    create_token("は", vec!["助詞", "係助詞"], 54, 57),
                    create_token("，", vec!["記号", "読点"], 57, 60),
                    create_token("これ", vec!["代名詞", "一般"], 60, 66),
                    create_token("は", vec!["助詞", "係助詞"], 66, 69),
                    create_token("，", vec!["記号", "読点"], 69, 72),
                    create_token("どう", vec!["副詞", "一般"], 72, 78),
                    create_token("です", vec!["助動詞"], 78, 84),
                    create_token("か", vec!["助詞", "終助詞"], 84, 87),
                    create_token("?", vec!["記号", "一般"], 87, 88),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"，\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "テスト文章において、テスト文章において、テスト文章において、テスト文章において、テスト文章において、テスト文章において、です".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 5
                }))
                .unwrap(),
                tokens: vec![
                    create_token("テスト", vec!["名詞", "一般"], 0, 3),
                    create_token("文章", vec!["名詞", "一般"], 3, 5),
                    create_token("において", vec!["助詞", "格助詞"], 5, 9),
                    create_token("、", vec!["記号", "読点"], 9, 10),
                    create_token("テスト", vec!["名詞", "一般"], 10, 13),
                    create_token("文章", vec!["名詞", "一般"], 13, 15),
                    create_token("において", vec!["助詞", "格助詞"], 15, 19),
                    create_token("、", vec!["記号", "読点"], 19, 20),
                    create_token("テスト", vec!["名詞", "一般"], 20, 23),
                    create_token("文章", vec!["名詞", "一般"], 23, 25),
                    create_token("において", vec!["助詞", "格助詞"], 25, 29),
                    create_token("、", vec!["記号", "読点"], 29, 30),
                    create_token("テスト", vec!["名詞", "一般"], 30, 33),
                    create_token("文章", vec!["名詞", "一般"], 33, 35),
                    create_token("において", vec!["助詞", "格助詞"], 35, 39),
                    create_token("、", vec!["記号", "読点"], 39, 40),
                    create_token("テスト", vec!["名詞", "一般"], 40, 43),
                    create_token("文章", vec!["名詞", "一般"], 43, 45),
                    create_token("において", vec!["助詞", "格助詞"], 45, 49),
                    create_token("、", vec!["記号", "読点"], 49, 50),
                    create_token("テスト", vec!["名詞", "一般"], 50, 53),
                    create_token("文章", vec!["名詞", "一般"], 53, 55),
                    create_token("において", vec!["助詞", "格助詞"], 55, 59),
                    create_token("、", vec!["記号", "読点"], 59, 60),
                    create_token("です", vec!["助動詞"], 60, 62),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を6つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "これは、長文の例ですが、columnがちゃんと計算、されてるはずです。".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 2
                }))
                .unwrap(),
                tokens: vec![
                    create_token("これ", vec!["代名詞", "一般"], 0, 2),
                    create_token("は", vec!["助詞", "係助詞"], 2, 3),
                    create_token("、", vec!["記号", "読点"], 3, 4),
                    create_token("長文", vec!["名詞", "一般"], 4, 6),
                    create_token("の", vec!["助詞", "連体化"], 6, 7),
                    create_token("例", vec!["名詞", "一般"], 7, 8),
                    create_token("です", vec!["助動詞"], 8, 10),
                    create_token("が", vec!["助詞", "接続助詞"], 10, 11),
                    create_token("、", vec!["記号", "読点"], 11, 12),
                    create_token("column", vec!["名詞", "一般"], 12, 18),
                    create_token("が", vec!["助詞", "格助詞"], 18, 19),
                    create_token("ちゃんと", vec!["副詞", "一般"], 19, 23),
                    create_token("計算", vec!["名詞", "サ変接続"], 23, 25),
                    create_token("、", vec!["記号", "読点"], 25, 26),
                    create_token("さ", vec!["動詞", "自立"], 26, 27),
                    create_token("れ", vec!["動詞", "接尾"], 27, 28),
                    create_token("てる", vec!["動詞", "非自立"], 28, 30),
                    create_token("はず", vec!["名詞", "非自立"], 30, 32),
                    create_token("です", vec!["助動詞"], 32, 34),
                    create_token("。", vec!["記号", "句点"], 34, 35),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を3つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "間に、Str以外の`code`Nodeが、あっても、OKと、聞いています。".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 3
                }))
                .unwrap(),
                tokens: vec![
                    create_token("間", vec!["名詞", "一般"], 0, 1),
                    create_token("に", vec!["助詞", "格助詞"], 1, 2),
                    create_token("、", vec!["記号", "読点"], 2, 3),
                    create_token("Str", vec!["名詞", "一般"], 3, 6),
                    create_token("以外", vec!["名詞", "一般"], 6, 8),
                    create_token("の", vec!["助詞", "連体化"], 8, 9),
                    create_token("`", vec!["記号", "一般"], 9, 10),
                    create_token("code", vec!["名詞", "一般"], 10, 14),
                    create_token("`", vec!["記号", "一般"], 14, 15),
                    create_token("Node", vec!["名詞", "一般"], 15, 19),
                    create_token("が", vec!["助詞", "格助詞"], 19, 20),
                    create_token("、", vec!["記号", "読点"], 20, 21),
                    create_token("あっ", vec!["動詞", "自立"], 21, 23),
                    create_token("て", vec!["助詞", "接続助詞"], 23, 24),
                    create_token("も", vec!["助詞", "係助詞"], 24, 25),
                    create_token("、", vec!["記号", "読点"], 25, 26),
                    create_token("OK", vec!["名詞", "一般"], 26, 28),
                    create_token("と", vec!["助詞", "格助詞"], 28, 29),
                    create_token("、", vec!["記号", "読点"], 29, 30),
                    create_token("聞い", vec!["動詞", "自立"], 30, 32),
                    create_token("て", vec!["助詞", "接続助詞"], 32, 33),
                    create_token("い", vec!["動詞", "非自立"], 33, 34),
                    create_token("ます", vec!["助動詞"], 34, 36),
                    create_token("。", vec!["記号", "句点"], 36, 37),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "複数のセンテンスがある場合。これでも、columnが、ちゃんと計算、されているはず、そのためのテキストです。".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 3
                }))
                .unwrap(),
                tokens: vec![
                    create_token("複数", vec!["名詞", "一般"], 0, 2),
                    create_token("の", vec!["助詞", "連体化"], 2, 3),
                    create_token("センテンス", vec!["名詞", "一般"], 3, 8),
                    create_token("が", vec!["助詞", "格助詞"], 8, 9),
                    create_token("ある", vec!["動詞", "自立"], 9, 11),
                    create_token("場合", vec!["名詞", "一般"], 11, 13),
                    create_token("。", vec!["記号", "句点"], 13, 14),
                    create_token("これ", vec!["代名詞", "一般"], 14, 16),
                    create_token("でも", vec!["助詞", "副助詞"], 16, 18),
                    create_token("、", vec!["記号", "読点"], 18, 19),
                    create_token("column", vec!["名詞", "一般"], 19, 25),
                    create_token("が", vec!["助詞", "格助詞"], 25, 26),
                    create_token("、", vec!["記号", "読点"], 26, 27),
                    create_token("ちゃんと", vec!["副詞", "一般"], 27, 31),
                    create_token("計算", vec!["名詞", "サ変接続"], 31, 33),
                    create_token("、", vec!["記号", "読点"], 33, 34),
                    create_token("さ", vec!["動詞", "自立"], 34, 35),
                    create_token("れ", vec!["動詞", "接尾"], 35, 36),
                    create_token("て", vec!["助詞", "接続助詞"], 36, 37),
                    create_token("いる", vec!["動詞", "非自立"], 37, 39),
                    create_token("はず", vec!["名詞", "非自立"], 39, 41),
                    create_token("、", vec!["記号", "読点"], 41, 42),
                    create_token("その", vec!["連体詞"], 42, 44),
                    create_token("ため", vec!["名詞", "非自立"], 44, 46),
                    create_token("の", vec!["助詞", "連体化"], 46, 47),
                    create_token("テキスト", vec!["名詞", "一般"], 47, 51),
                    create_token("です", vec!["助動詞"], 51, 53),
                    create_token("。", vec!["記号", "句点"], 53, 54),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "複数のセンテンスがあって、改行されている場合でも\n大丈夫です。これでも、lineとcolumnが、ちゃんと計算、されているはず、そのためのテキストです。".to_string(),
                config: serde_json::from_value(serde_json::json!({
                    "max": 3
                }))
                .unwrap(),
                tokens: vec![
                    create_token("複数", vec!["名詞", "一般"], 0, 2),
                    create_token("の", vec!["助詞", "連体化"], 2, 3),
                    create_token("センテンス", vec!["名詞", "一般"], 3, 8),
                    create_token("が", vec!["助詞", "格助詞"], 8, 9),
                    create_token("あっ", vec!["動詞", "自立"], 9, 11),
                    create_token("て", vec!["助詞", "接続助詞"], 11, 12),
                    create_token("、", vec!["記号", "読点"], 12, 13),
                    create_token("改行", vec!["名詞", "サ変接続"], 13, 15),
                    create_token("さ", vec!["動詞", "自立"], 15, 16),
                    create_token("れ", vec!["動詞", "接尾"], 16, 17),
                    create_token("て", vec!["助詞", "接続助詞"], 17, 18),
                    create_token("いる", vec!["動詞", "非自立"], 18, 20),
                    create_token("場合", vec!["名詞", "非自立"], 20, 22),
                    create_token("でも", vec!["助詞", "副助詞"], 22, 24),
                    create_token("\n", vec!["記号", "空白"], 24, 25),
                    create_token("大丈夫", vec!["名詞", "形容動詞語幹"], 25, 28),
                    create_token("です", vec!["助動詞"], 28, 30),
                    create_token("。", vec!["記号", "句点"], 30, 31),
                    create_token("これ", vec!["代名詞", "一般"], 31, 33),
                    create_token("でも", vec!["助詞", "副助詞"], 33, 35),
                    create_token("、", vec!["記号", "読点"], 35, 36),
                    create_token("line", vec!["名詞", "一般"], 36, 40),
                    create_token("と", vec!["助詞", "並立助詞"], 40, 41),
                    create_token("column", vec!["名詞", "一般"], 41, 47),
                    create_token("が", vec!["助詞", "格助詞"], 47, 48),
                    create_token("、", vec!["記号", "読点"], 48, 49),
                    create_token("ちゃんと", vec!["副詞", "一般"], 49, 53),
                    create_token("計算", vec!["名詞", "サ変接続"], 53, 55),
                    create_token("、", vec!["記号", "読点"], 55, 56),
                    create_token("さ", vec!["動詞", "自立"], 56, 57),
                    create_token("れ", vec!["動詞", "接尾"], 57, 58),
                    create_token("て", vec!["助詞", "接続助詞"], 58, 59),
                    create_token("いる", vec!["動詞", "非自立"], 59, 61),
                    create_token("はず", vec!["名詞", "非自立"], 61, 63),
                    create_token("、", vec!["記号", "読点"], 63, 64),
                    create_token("その", vec!["連体詞"], 64, 66),
                    create_token("ため", vec!["名詞", "非自立"], 66, 68),
                    create_token("の", vec!["助詞", "連体化"], 68, 69),
                    create_token("テキスト", vec!["名詞", "一般"], 69, 73),
                    create_token("です", vec!["助動詞"], 73, 75),
                    create_token("。", vec!["記号", "句点"], 75, 76),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
            TestCase {
                text: "このパラグラフはOKです。\n            \n変数の名前は、名前のルールが決まっていて、そのルールは複雑で、たくさんのルールがあるので、箇条書きしましょう。\n\n3つめのパラグラフはもOKです。\n\n".to_string(),
                config: None,
                tokens: vec![
                    create_token("この", vec!["連体詞"], 0, 2),
                    create_token("パラグラフ", vec!["名詞", "一般"], 2, 7),
                    create_token("は", vec!["助詞", "係助詞"], 7, 8),
                    create_token("OK", vec!["名詞", "一般"], 8, 10),
                    create_token("です", vec!["助動詞"], 10, 12),
                    create_token("。", vec!["記号", "句点"], 12, 13),
                    create_token("\n            \n", vec!["記号", "空白"], 13, 27),
                    create_token("変数", vec!["名詞", "一般"], 27, 29),
                    create_token("の", vec!["助詞", "連体化"], 29, 30),
                    create_token("名前", vec!["名詞", "一般"], 30, 32),
                    create_token("は", vec!["助詞", "係助詞"], 32, 33),
                    create_token("、", vec!["記号", "読点"], 33, 34),
                    create_token("名前", vec!["名詞", "一般"], 34, 36),
                    create_token("の", vec!["助詞", "連体化"], 36, 37),
                    create_token("ルール", vec!["名詞", "一般"], 37, 40),
                    create_token("が", vec!["助詞", "格助詞"], 40, 41),
                    create_token("決まっ", vec!["動詞", "自立"], 41, 44),
                    create_token("て", vec!["助詞", "接続助詞"], 44, 45),
                    create_token("い", vec!["動詞", "非自立"], 45, 46),
                    create_token("て", vec!["助詞", "接続助詞"], 46, 47),
                    create_token("、", vec!["記号", "読点"], 47, 48),
                    create_token("その", vec!["連体詞"], 48, 50),
                    create_token("ルール", vec!["名詞", "一般"], 50, 53),
                    create_token("は", vec!["助詞", "係助詞"], 53, 54),
                    create_token("複雑", vec!["名詞", "形容動詞語幹"], 54, 56),
                    create_token("で", vec!["助動詞"], 56, 57),
                    create_token("、", vec!["記号", "読点"], 57, 58),
                    create_token("たくさん", vec!["名詞", "副詞可能"], 58, 62),
                    create_token("の", vec!["助詞", "連体化"], 62, 63),
                    create_token("ルール", vec!["名詞", "一般"], 63, 66),
                    create_token("が", vec!["助詞", "格助詞"], 66, 67),
                    create_token("ある", vec!["動詞", "自立"], 67, 69),
                    create_token("ので", vec!["助詞", "接続助詞"], 69, 71),
                    create_token("、", vec!["記号", "読点"], 71, 72),
                    create_token("箇条書き", vec!["名詞", "一般"], 72, 76),
                    create_token("し", vec!["動詞", "自立"], 76, 77),
                    create_token("ましょう", vec!["助動詞"], 77, 81),
                    create_token("。", vec!["記号", "句点"], 81, 82),
                    create_token("\n\n", vec!["記号", "空白"], 82, 84),
                    create_token("3", vec!["名詞", "数"], 84, 85),
                    create_token("つ", vec!["名詞", "接尾"], 85, 86),
                    create_token("め", vec!["名詞", "接尾"], 86, 87),
                    create_token("の", vec!["助詞", "連体化"], 87, 88),
                    create_token("パラグラフ", vec!["名詞", "一般"], 88, 93),
                    create_token("は", vec!["助詞", "係助詞"], 93, 94),
                    create_token("も", vec!["助詞", "係助詞"], 94, 95),
                    create_token("OK", vec!["名詞", "一般"], 95, 97),
                    create_token("です", vec!["助動詞"], 97, 99),
                    create_token("。", vec!["記号", "句点"], 99, 100),
                    create_token("\n\n", vec!["記号", "空白"], 100, 102),
                ],
                expected_errors: vec![ExpectedError {
                    message: "一つの文で\"、\"を4つ以上使用しています".to_string(),
                }],
            },
        ];
        run_test_cases(valid, invalid);
    }
}
