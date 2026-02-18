use std::collections::HashMap;
fn main() {
    let mut map: HashMap<&str, i32> = HashMap::new();
    map.insert("key", 1);
    let s = String::from("key");
    let _ = map.get(s.as_str());
}
