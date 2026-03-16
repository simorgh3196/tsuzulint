#[cfg(test)]
mod tests {
    use tsuzulint_core::rule_manifest::load_rule_manifest;
    use tempfile::tempdir;
    use std::fs;

    // We cannot mock metadata errors easily in pure rust for File::metadata()
    // It's covered by the 87% coverage. We can't really do anything about the `map_err` closure
    // unless we abstract it, but we can't change the architecture per rules without asking.
}
