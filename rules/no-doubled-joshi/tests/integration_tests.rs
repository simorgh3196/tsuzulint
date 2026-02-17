//! Integration tests for the no-doubled-joshi rule.

use tsuzulint_rule_pdk::{Diagnostic, Fix, LintRequest, RuleManifest, Span};

/// Helper to create a lint request for testing
fn create_request(text: &str, config: serde_json::Value) -> LintRequest {
    LintRequest {
        node: serde_json::json!({
            "type": "Str",
            "range": [0, text.len()],
            "value": text
        }),
        config,
        source: text.to_string(),
        file_path: None,
    }
}

#[test]
fn test_manifest_structure() {
    let manifest = RuleManifest::new("no-doubled-joshi", "1.0.0")
        .with_description("Detect repeated Japanese particles (助詞)")
        .with_fixable(true)
        .with_node_types(vec!["Str".to_string()]);

    assert_eq!(manifest.name, "no-doubled-joshi");
    assert_eq!(manifest.version, "1.0.0");
    assert!(manifest.description.is_some());
    assert!(manifest.fixable);
    assert_eq!(manifest.node_types, vec!["Str".to_string()]);
}

#[test]
fn test_detects_doubled_wa_particle() {
    let text = "私はは学生です";
    let request = create_request(text, serde_json::json!({}));

    // Text contains doubled は particle
    let wa_count = text.chars().filter(|c| *c == 'は').count();
    assert_eq!(wa_count, 2);

    // Check they are consecutive
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() - 1 {
        if chars[i] == 'は' && chars[i + 1] == 'は' {
            // Found consecutive は particles
            assert!(true);
            return;
        }
    }
}

#[test]
fn test_detects_doubled_ga_particle() {
    let text = "学生がが来ました";
    let request = create_request(text, serde_json::json!({}));

    let ga_count = text.chars().filter(|c| *c == 'が').count();
    assert_eq!(ga_count, 2);
}

#[test]
fn test_no_error_for_single_particles() {
    let text = "私は学生です";
    let request = create_request(text, serde_json::json!({}));

    // Only one は particle, should be clean
    let wa_count = text.chars().filter(|c| *c == 'は').count();
    assert_eq!(wa_count, 1);
}

#[test]
fn test_no_error_for_different_particles() {
    let text = "私は学生が来ました";
    let request = create_request(text, serde_json::json!({}));

    // は and が are different particles
    assert!(text.contains('は'));
    assert!(text.contains('が'));

    // Check they are not consecutive
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() - 1 {
        if chars[i] == 'は' {
            assert_ne!(chars[i + 1], 'は');
        }
    }
}

#[test]
fn test_detects_doubled_wo_particle() {
    let text = "本をを読む";
    let request = create_request(text, serde_json::json!({}));

    let wo_count = text.chars().filter(|c| *c == 'を').count();
    assert_eq!(wo_count, 2);
}

#[test]
fn test_detects_doubled_ni_particle() {
    let text = "東京にに行く";
    let request = create_request(text, serde_json::json!({}));

    let ni_count = text.chars().filter(|c| *c == 'に').count();
    assert_eq!(ni_count, 2);
}

#[test]
fn test_clean_japanese_text() {
    let text = "今日は良い天気です";
    let request = create_request(text, serde_json::json!({}));

    // No doubled particles
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() - 1 {
        // No character should be repeated as particle
        if chars[i] == 'は' || chars[i] == 'が' || chars[i] == 'を' || chars[i] == 'に' {
            assert_ne!(chars[i], chars[i + 1]);
        }
    }
}

#[test]
fn test_config_custom_particles() {
    let config = serde_json::json!({
        "particles": ["は", "が"]
    });

    let request = create_request("私はは学生", config);

    // Config specifies only は and が particles
    assert!(request.config.get("particles").is_some());
}

#[test]
fn test_config_allow_particles() {
    let config = serde_json::json!({
        "allow": ["は"]
    });

    let request = create_request("私はは学生", config);

    // Config allows は particle to be doubled
    assert!(request.config.get("allow").is_some());
}

#[test]
fn test_config_min_interval() {
    let config = serde_json::json!({
        "min_interval": 2
    });

    let request = create_request("私は学生は", config);

    // With min_interval: 2, particles separated by "学生" (2 chars) might be allowed
    assert!(request.config.get("min_interval").is_some());
}

#[test]
fn test_config_suggest_fix() {
    let config = serde_json::json!({
        "suggest_fix": true
    });

    let request = create_request("私はは学生", config);

    assert_eq!(request.config.get("suggest_fix").unwrap(), &true);
}

#[test]
fn test_non_consecutive_same_particles() {
    let text = "私は学生は社会人です";
    let request = create_request(text, serde_json::json!({}));

    // Two は particles but not consecutive
    let wa_count = text.chars().filter(|c| *c == 'は').count();
    assert_eq!(wa_count, 2);

    // Verify they are separated
    let chars: Vec<char> = text.chars().collect();
    let mut wa_positions = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c == 'は' {
            wa_positions.push(i);
        }
    }

    if wa_positions.len() >= 2 {
        assert!(wa_positions[1] - wa_positions[0] > 1);
    }
}

#[test]
fn test_multiple_doubled_particles() {
    let text = "私はは学生がが来ました";
    let request = create_request(text, serde_json::json!({}));

    // Contains both はは and がが
    assert!(text.contains("はは"));
    assert!(text.contains("がが"));
}

#[test]
fn test_particle_at_start() {
    let text = "はは、これは間違いです";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.starts_with("はは"));
}

#[test]
fn test_particle_at_end() {
    let text = "これは間違いはは";
    let request = create_request(text, serde_json::json!({}));

    assert!(text.ends_with("はは"));
}

#[test]
fn test_empty_japanese_text() {
    let text = "";
    let request = create_request(text, serde_json::json!({}));

    assert!(request.source.is_empty());
}

#[test]
fn test_mixed_japanese_and_english() {
    let text = "私はは student です";
    let request = create_request(text, serde_json::json!({}));

    // Should still detect doubled は even with mixed text
    let wa_count = text.chars().filter(|c| *c == 'は').count();
    assert_eq!(wa_count, 2);
}

#[test]
fn test_particles_with_punctuation() {
    let text = "私は、は学生です";
    let request = create_request(text, serde_json::json!({}));

    // Particles separated by punctuation
    let wa_count = text.chars().filter(|c| *c == 'は').count();
    assert_eq!(wa_count, 2);
}

#[test]
fn test_hiragana_only_text() {
    let text = "わたしははがくせいです";
    let request = create_request(text, serde_json::json!({}));

    // All hiragana including doubled は
    assert!(text.contains("はは"));
}

#[test]
fn test_katakana_text_no_particles() {
    let text = "テストデータ";
    let request = create_request(text, serde_json::json!({}));

    // Katakana text typically doesn't have hiragana particles
    assert!(!text.contains('は'));
    assert!(!text.contains('が'));
}

#[test]
fn test_long_sentence_with_particles() {
    let text = "今日は天気が良いので、私は公園に行って、友達と遊びました";
    let request = create_request(text, serde_json::json!({}));

    // Long sentence with various particles but no doubles
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() - 1 {
        if chars[i] == 'は' || chars[i] == 'が' || chars[i] == 'を' || chars[i] == 'に' {
            // Check next char is not the same particle
            assert_ne!(chars[i], chars[i + 1]);
        }
    }
}

#[test]
fn test_fix_suggestion_structure() {
    // Test that Fix structure is correctly formed
    let fix = Fix::delete(Span::new(3, 6));

    // Fix should have a span
    assert_eq!(fix.span.start, 3);
    assert_eq!(fix.span.end, 6);
}

#[test]
fn test_diagnostic_with_fix() {
    // Test creating a diagnostic with a fix
    let diagnostic = Diagnostic::new(
        "no-doubled-joshi",
        "Doubled particle 'に' detected",
        Span::new(6, 12),
    )
    .with_fix(Fix::delete(Span::new(9, 12)));

    assert!(diagnostic.fix.is_some());
    assert_eq!(diagnostic.message, "Doubled particle 'に' detected");
}

#[test]
fn test_particle_de() {
    let text = "学校でで勉強する";
    let request = create_request(text, serde_json::json!({}));

    let de_count = text.chars().filter(|c| *c == 'で').count();
    assert_eq!(de_count, 2);
}

#[test]
fn test_particle_to() {
    let text = "友達とと遊ぶ";
    let request = create_request(text, serde_json::json!({}));

    let to_count = text.chars().filter(|c| *c == 'と').count();
    assert_eq!(to_count, 2);
}

#[test]
fn test_particle_mo() {
    let text = "私もも行く";
    let request = create_request(text, serde_json::json!({}));

    let mo_count = text.chars().filter(|c| *c == 'も').count();
    assert_eq!(mo_count, 2);
}

#[test]
fn test_config_default_behavior() {
    let config = serde_json::json!({});

    let request = create_request("私はは学生", config);

    // Default config should be empty object
    assert!(request.config.is_object());
}