use std::fs::File;
use std::io::Read;
fn main() {
    let mut file = match File::open(".") {
        Ok(f) => f,
        Err(e) => {
            println!("open failed: {}", e);
            return;
        }
    };
    let mut content = String::new();
    match file.read_to_string(&mut content) {
        Ok(_) => println!("read ok"),
        Err(e) => println!("read failed: {}", e),
    }
}
