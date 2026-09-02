use std::env;
use tracing::warn;

/// Read the desired port from the PORT environment variable.
/// Returns None if unset or invalid; caller should bind to port 0 to get an ephemeral port.
pub fn port_from_env() -> Option<u16> {
    match env::var("PORT") {
        Ok(val) => match val.parse::<u16>() {
            Ok(p) => Some(p),
            Err(_) => {
                warn!(
                    "Invalid PORT value {:?}; falling back to ephemeral port",
                    val
                );
                None
            }
        },
        Err(_) => None,
    }
}

pub fn admin_credentials_from_env() -> Result<(String, String), std::env::VarError> {
    let user = env::var("ADMIN_USER")?;
    let pass = env::var("ADMIN_PASS")?;
    Ok((user, pass))
}
