//! Corpus generator for benchmark tests.
//!
//! Generates test files on-demand. Generated files are NOT committed to git.
//! Run: cargo run --bin corpus-generator

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

const CORPUS_DIR: &str = "benches/corpus";
const SINGLE_LARGE_FILE: &str = "large_single.md";
const MANY_FILES_DIR: &str = "many_files";

/// Content templates for generating realistic markdown.
const PARAGRAPHS: &[&str] = &[
    "This is a sample paragraph for testing purposes. It contains enough text to be realistic.",
    "TODO: Remember to update this documentation before the next release.",
    "The quick brown fox jumps over the lazy dog. This is a classic pangram used for testing.",
    "FIXME: This section needs better error handling examples.",
    "Rust is a systems programming language that runs blazingly fast, prevents segfaults, and guarantees thread safety.",
    "XXX: Review this code with the team next week.",
    "Markdown is a lightweight markup language with plain-text formatting syntax.",
    "Performance optimization is crucial for developer tools that process large codebases.",
];

const CODE_BLOCKS: &[&str] = &[
    "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```",
    "```javascript\nfunction greet() {\n    return 'Hello';\n}\n```",
    "```python\ndef main():\n    print(\"Hello, world!\")\n```",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating benchmark corpus...");

    // Create corpus directory
    fs::create_dir_all(CORPUS_DIR)?;

    // Generate 100MB single file
    generate_large_file()?;

    // Generate 1000 small files
    generate_many_files()?;

    println!("Corpus generation complete!");
    println!(
        "  - Large file: {}/{} (~100MB)",
        CORPUS_DIR, SINGLE_LARGE_FILE
    );
    println!(
        "  - Many files: {}/{} (1000 files)",
        CORPUS_DIR, MANY_FILES_DIR
    );

    Ok(())
}

/// Generates a ~100MB markdown file.
fn generate_large_file() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(CORPUS_DIR).join(SINGLE_LARGE_FILE);
    let mut file = File::create(&path)?;

    let target_size = 100 * 1024 * 1024; // 100MB
    let mut current_size = 0;
    let mut file_count = 0;

    writeln!(file, "# Large Test File\n")?;
    writeln!(
        file,
        "This file is approximately 100MB for benchmark testing.\n"
    )?;

    while current_size < target_size {
        file_count += 1;
        writeln!(file, "## Section {}\n", file_count)?;

        // Add paragraphs
        for (i, para) in PARAGRAPHS.iter().enumerate() {
            writeln!(file, "{}", para)?;
            if i % 3 == 2 {
                writeln!(file)?; // Empty line every 3 paragraphs
            }
            current_size += para.len();
        }

        // Add code blocks
        if file_count % 5 == 0 {
            for code in CODE_BLOCKS {
                writeln!(file, "\n{}", code)?;
                current_size += code.len();
            }
        }

        // Add a list
        writeln!(file, "\n### Checklist for section {}", file_count)?;
        for i in 1..=5 {
            writeln!(file, "- [ ] Task {}.{}", file_count, i)?;
            current_size += 20;
        }
        writeln!(file)?;

        // Progress indicator
        if file_count % 100 == 0 {
            let mb = current_size / (1024 * 1024);
            print!("\r  Generated: {}MB / 100MB", mb);
            std::io::stdout().flush()?;
        }
    }

    println!("\r  Generated: 100MB / 100MB ✓");

    let actual_size = fs::metadata(&path)?.len();
    println!(
        "  Actual size: {:.2} MB",
        actual_size as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

/// Generates 1000 small markdown files.
fn generate_many_files() -> Result<(), Box<dyn std::error::Error>> {
    let dir_path = Path::new(CORPUS_DIR).join(MANY_FILES_DIR);
    fs::create_dir_all(&dir_path)?;

    for i in 1..=1000 {
        let filename = format!("doc_{:04}.md", i);
        let path = dir_path.join(&filename);
        let mut file = File::create(&path)?;

        writeln!(file, "# Document {}\n", i)?;
        writeln!(
            file,
            "This is test document number {} for benchmark testing.\n",
            i
        )?;

        // Add 3-5 paragraphs
        let num_paragraphs = 3 + (i % 3);
        for j in 0..num_paragraphs {
            let para = PARAGRAPHS[j % PARAGRAPHS.len()];
            writeln!(file, "{}", para)?;
            writeln!(file)?;
        }

        // Add a code block for every 10th file
        if i % 10 == 0 {
            let code = CODE_BLOCKS[i % CODE_BLOCKS.len()];
            writeln!(file, "{}", code)?;
            writeln!(file)?;
        }

        // Progress indicator
        if i % 100 == 0 {
            print!("\r  Generated: {}/1000 files", i);
            std::io::stdout().flush()?;
        }
    }

    println!("\r  Generated: 1000/1000 files ✓");

    Ok(())
}
