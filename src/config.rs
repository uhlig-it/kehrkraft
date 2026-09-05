use std::env;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Read the desired port from the KEHRKRAFT_PORT environment variable.
/// Returns None if unset or invalid; caller should bind to port 0 to get an ephemeral port.
pub fn port_from_env() -> Option<u16> {
    match env::var("KEHRKRAFT_PORT") {
        Ok(val) => match val.parse::<u16>() {
            Ok(p) => Some(p),
            Err(_) => {
                warn!(
                    "Invalid KEHRKRAFT_PORT value {:?}; falling back to ephemeral port",
                    val
                );
                None
            }
        },
        Err(_) => None,
    }
}

/// Path of the file that persists the automatically assigned dev port across
/// restarts (e.g. under `cargo watch`). Override with KEHRKRAFT_PORT_FILE.
pub fn port_file_path() -> PathBuf {
    env::var_os("KEHRKRAFT_PORT_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".kehrkraft-port"))
}

/// Read the dev port previously persisted via [`save_dev_port`], if any.
pub fn saved_dev_port(path: &Path) -> Option<u16> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Persist the dev port so that restarts (e.g. under `cargo watch`) reuse it.
pub fn save_dev_port(path: &Path, port: u16) {
    if let Err(err) = std::fs::write(path, port.to_string()) {
        warn!(
            "Cannot save dev port {} to {}: {}",
            port,
            path.display(),
            err
        );
    }
}

pub fn admin_credentials_from_env() -> Result<(String, String), std::env::VarError> {
    let user = env::var("KEHRKRAFT_ADMIN_USER")?;
    let pass = env::var("KEHRKRAFT_ADMIN_PASS")?;
    Ok((user, pass))
}

/// Whether to run in demo mode (KEHRKRAFT_DEMO_MODE=true).
/// Demo mode disables authentication and shows a "Demo Mode" banner.
pub fn demo_mode_from_env() -> bool {
    match env::var("KEHRKRAFT_DEMO_MODE") {
        Ok(val) => {
            let normalized = val.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "true" | "1" | "yes" | "on")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_port_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kehrkraft-port-{}-{}.tmp",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn save_and_read_roundtrip() {
        let path = temp_port_file("roundtrip");
        save_dev_port(&path, 54321);
        assert_eq!(saved_dev_port(&path), Some(54321));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_file_yields_none() {
        let path = temp_port_file("missing");
        std::fs::remove_file(&path).ok();
        assert_eq!(saved_dev_port(&path), None);
    }

    #[test]
    fn invalid_content_yields_none() {
        let path = temp_port_file("invalid");
        std::fs::write(&path, "not-a-port\n").unwrap();
        assert_eq!(saved_dev_port(&path), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn overwrite_replaces_previous_port() {
        let path = temp_port_file("overwrite");
        save_dev_port(&path, 10000);
        save_dev_port(&path, 20000);
        assert_eq!(saved_dev_port(&path), Some(20000));
        std::fs::remove_file(&path).ok();
    }
}
