use std::{io, path::Path};

/// Bundle resource mappings place both files directly beside the installed exe.
pub fn validate_resources(directory: &Path, expected_version: &str) -> io::Result<()> {
    let binary = std::fs::metadata(directory.join("devbox-linux"))?;
    if !binary.is_file() || binary.len() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Linux binary is missing or empty",
        ));
    }
    let version = std::fs::read_to_string(directory.join("devbox-version.txt"))?;
    if version.trim() != expected_version {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bundled version does not match app version",
        ));
    }
    Ok(())
}

pub fn validate_channel(arguments: &[String]) -> io::Result<()> {
    if let Some(index) = arguments.iter().position(|arg| arg == "--channel")
        && arguments.get(index + 1).map(String::as_str)
            != Some(super::channel::CHANNEL.config().instance)
    {
        return Err(io::Error::other("self-check requested a different channel"));
    }
    Ok(())
}

pub fn run() -> i32 {
    let result = std::env::current_exe().and_then(|exe| {
        let directory = exe
            .parent()
            .ok_or_else(|| io::Error::other("executable has no parent"))?;
        validate_channel(&std::env::args().collect::<Vec<_>>())?;
        #[cfg(windows)]
        {
            let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
            let config = context.config();
            let protocols = &config.plugins.0["deep-link"]["desktop"]["schemes"];
            let schemes: Vec<_> = protocols
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .collect();
            if !super::channel::CHANNEL.config().matches_tauri(
                &config.identifier,
                config.product_name.as_deref().unwrap_or_default(),
                &schemes,
            ) {
                return Err(io::Error::other("compiled channel/Tauri identity mismatch"));
            }
        }
        validate_resources(directory, env!("CARGO_PKG_VERSION"))
    });
    match result {
        Ok(()) => {
            println!("devbox self-check: ok ({})", env!("CARGO_PKG_VERSION"));
            0
        }
        Err(error) => {
            eprintln!("devbox self-check: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_channel_must_match_compiled_channel() {
        for channel in ["prod", "accept", "dev", "invalid"] {
            assert_eq!(
                validate_channel(&["--channel".into(), channel.into()]).is_ok(),
                channel == super::super::channel::CHANNEL.config().instance
            );
        }
        assert!(validate_channel(&["--channel".into()]).is_err());
    }

    #[test]
    fn requires_binary_and_matching_version_beside_executable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
        std::fs::write(path.join("devbox-version.txt"), "1.0.0-dev\n").unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
        std::fs::write(path.join("devbox-linux"), b"owned test fixture").unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_ok());
        assert!(validate_resources(path, "1.0.1").is_err());
        std::fs::remove_file(path.join("devbox-version.txt")).unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
    }

    #[test]
    fn rejects_empty_binary_directory_and_invalid_version_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        std::fs::write(path.join("devbox-version.txt"), "1.0.0-dev").unwrap();
        std::fs::write(path.join("devbox-linux"), b"").unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
        std::fs::remove_file(path.join("devbox-linux")).unwrap();
        std::fs::create_dir(path.join("devbox-linux")).unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
        std::fs::remove_dir(path.join("devbox-linux")).unwrap();
        std::fs::write(path.join("devbox-linux"), b"fixture").unwrap();
        std::fs::write(path.join("devbox-version.txt"), [0xff]).unwrap();
        assert!(validate_resources(path, "1.0.0-dev").is_err());
    }
}
