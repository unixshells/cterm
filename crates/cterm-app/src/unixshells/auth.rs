//! Ed25519 key management and auth token signing for Unix Shells.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Load an existing relay key or generate a new Ed25519 key.
pub fn load_or_generate_relay_key(config_dir: &Path) -> anyhow::Result<ssh_key::PrivateKey> {
    let path = config_dir.join("relay.key");

    if path.exists() {
        let key = ssh_key::PrivateKey::read_openssh_file(&path)
            .map_err(|e| anyhow::anyhow!("Failed to load relay key {}: {}", path.display(), e))?;
        return Ok(key);
    }

    // Generate new Ed25519 key
    let key =
        ssh_key::PrivateKey::random(&mut ssh_key::rand_core::OsRng, ssh_key::Algorithm::Ed25519)
            .map_err(|e| anyhow::anyhow!("Failed to generate relay key: {}", e))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    key.write_openssh_file(&path, ssh_key::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Failed to write relay key: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    log::info!("Generated new relay key at {}", path.display());
    Ok(key)
}

/// Sign an auth token for the Unix Shells API.
///
/// Returns a string in the format `"{timestamp_ms}:{base64(signature)}"`.
/// The server verifies by checking the signature against the registered
/// public key and that the timestamp is within 60 seconds.
pub fn sign_auth_token(key: &ssh_key::PrivateKey) -> anyhow::Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("System time error: {}", e))?
        .as_millis();

    let timestamp_str = timestamp.to_string();

    // Extract Ed25519 signing key
    let ed_key = match key.key_data() {
        ssh_key::private::KeypairData::Ed25519(kp) => kp,
        _ => return Err(anyhow::anyhow!("Relay key must be Ed25519")),
    };

    let signing_key = ed25519_dalek::SigningKey::from_bytes(&ed_key.private.to_bytes());
    use ed25519_dalek::Signer;
    let signature = signing_key.sign(timestamp_str.as_bytes());

    use base64::Engine;
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

    Ok(format!("{}:{}", timestamp_str, sig_b64))
}

/// Get the public key in OpenSSH format (e.g., "ssh-ed25519 AAAA... cterm").
pub fn public_key_openssh(key: &ssh_key::PrivateKey) -> anyhow::Result<String> {
    let pubkey = key.public_key();
    let mut openssh = pubkey
        .to_openssh()
        .map_err(|e| anyhow::anyhow!("Failed to encode public key: {}", e))?;
    // Append comment
    openssh.push_str(" cterm");
    Ok(openssh)
}

/// Get the device name for this machine.
pub fn device_name() -> String {
    if let Ok(hostname) = hostname::get() {
        let name = hostname.to_string_lossy().to_string();
        if !name.is_empty() {
            return name;
        }
    }
    format!("cterm-{}", std::env::consts::OS)
}
