use std::collections::HashMap;

fn main() {
    let rules = vec!["A", "B"];
    let mut in_degree: HashMap<&str, usize> = rules.iter().map(|r| (*r, 0)).collect();

    *in_degree.entry("A").or_default() += 1;
    println!("{:?}", in_degree);
}
