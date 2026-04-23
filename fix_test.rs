fn main() {
    let toml = "[cache]\nenabled = true\ndirectory = \"/tmp\"\n";
    let mut config = wasmtime::Config::new();
    let temp = std::env::temp_dir().join("test.toml");
    std::fs::write(&temp, toml).unwrap();
    match config.cache_config_load(&temp) {
        Ok(_) => println!("ok"),
        Err(e) => println!("err: {:?}", e)
    }
}
