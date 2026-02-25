//! Init command implementation

use std::path::PathBuf;

use miette::{IntoDiagnostic, Result};
use tracing::info;
use tsuzulint_core::LinterConfig;

pub fn run_init(force: bool) -> Result<()> {
    let config_path = PathBuf::from(LinterConfig::CONFIG_FILES[0]);

    let default_config = r#"{
  "rules": [],
  "options": {},
  "cache": true
}
"#;

    loop {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }

        match options.open(&config_path) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(default_config.as_bytes())
                    .into_diagnostic()?;
                info!("Created {}", config_path.display());
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !force {
                    return Err(miette::miette!(
                        "Config file already exists. Use --force to overwrite."
                    ));
                }

                match std::fs::remove_file(&config_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e).into_diagnostic(),
                }
            }
            Err(e) => return Err(e).into_diagnostic(),
        }
    }
}
