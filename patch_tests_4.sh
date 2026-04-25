sed -i 's/let mut file = tempfile::NamedTempFile::new().unwrap();//g' crates/tsuzulint_cli/src/utils/mod.rs
sed -i 's/use std::io::Write;//g' crates/tsuzulint_cli/src/utils/mod.rs
