use std::fs::File;

fn main() {
    let file = File::open("/dev/zero").unwrap();
    println!("len: {}", file.metadata().unwrap().len());
}
