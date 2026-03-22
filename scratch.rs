use std::fs::File;
use std::io::Read;

fn main() {
    let mut f = File::open("/dev/urandom").unwrap();
    let mut s = String::new();
    match f.read_to_string(&mut s) {
        Err(e) => println!("Error: {}", e),
        _ => (),
    }
}
