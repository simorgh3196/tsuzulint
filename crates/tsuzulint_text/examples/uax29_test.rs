use unicode_segmentation::UnicodeSegmentation;

fn main() {
    let inputs = vec![
        "すごい！！本当に！？",          // No space
        "すごい！！ 本当に！？",         // With space
        "Hello!World",                   // No space (English)
        "Hello! World",                  // With space (English)
        "こんにちは。元気？",            // Japanese Kuten + Question
        "Mr. Smith went to Washington.", // Abbreviation
        "3.14 is pi.",                   // Number
        "This is ver.1.0.",              // Version number
        "Yahoo! JAPAN",                  // Brand name with space
        "Yahoo!JAPAN",                   // Brand name without space
    ];

    for text in inputs {
        println!("--- Input: '{}' ---", text);
        let sentences: Vec<&str> = text.unicode_sentences().collect();
        for (i, s) in sentences.iter().enumerate() {
            println!("  {}: '{}'", i, s);
        }
    }
}
