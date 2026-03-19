//! Start mosh-server via SSH and parse connection info.

use crate::{MoshConnectionType, MoshError};

/// Connection info returned by mosh-server.
#[derive(Debug, Clone)]
pub struct MoshConnectInfo {
    /// UDP port to connect to
    pub port: u16,
    /// Base64-encoded AES-128 key
    pub key: String,
    /// Optional IP address (from MOSH IP line)
    pub ip: Option<String>,
    /// Resolved IP of the jump host (for relay UDP fallback)
    pub jump_host_address: Option<String>,
}

/// Launch mosh-server on a remote host via SSH (direct connection).
///
/// Runs: `ssh [-J proxy] host "mosh-server new -s -c 256 -l LANG=... -l TERM=..."`
/// Parses stdout for `MOSH CONNECT <port> <key>` and optional `MOSH IP <addr>`.
pub async fn launch_mosh_server(
    host: &str,
    connection_type: &MoshConnectionType,
    proxy_jump: Option<&str>,
    locale: Option<&str>,
    term: Option<&str>,
    extra_ssh_args: &[String],
) -> Result<MoshConnectInfo, MoshError> {
    match connection_type {
        MoshConnectionType::Direct => {
            launch_direct(host, proxy_jump, locale, term, extra_ssh_args).await
        }
        MoshConnectionType::Relay {
            relay_host,
            jump_host,
            relay_username,
            relay_device,
            session_name,
        } => {
            launch_relay(
                relay_host,
                jump_host,
                relay_username,
                relay_device,
                session_name,
                locale,
                term,
                extra_ssh_args,
            )
            .await
        }
    }
}

/// Direct SSH connection to launch mosh-server.
async fn launch_direct(
    host: &str,
    proxy_jump: Option<&str>,
    locale: Option<&str>,
    term: Option<&str>,
    extra_ssh_args: &[String],
) -> Result<MoshConnectInfo, MoshError> {
    let mut cmd = tokio::process::Command::new("ssh");

    if let Some(proxy) = proxy_jump {
        cmd.arg("-J").arg(proxy);
    }

    for arg in extra_ssh_args {
        cmd.arg(arg);
    }

    cmd.arg("-tt"); // Force PTY allocation (triggers PAM/MOTD like the mobile app)
    cmd.arg(host);
    cmd.arg(build_mosh_server_cmd(locale, term));

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    log::debug!("launching mosh-server via direct SSH on {}", host);

    let output = run_ssh_command(&mut cmd).await?;
    let combined = format!("{}{}", output.0, output.1);
    parse_mosh_output(&combined)
}

/// Relay SSH connection: 3-hop tunnel through jump host.
///
/// Equivalent to the mobile app's flow:
/// 1. SSH to jump_host as user "jump"
/// 2. Tunnel (direct-tcpip) to {device}.{username}.{relay_host}:22
/// 3. SSH through tunnel as username = session_name
/// 4. Execute mosh-server
///
/// With OpenSSH, this is: `ssh -o ProxyCommand="ssh -W %h:%p jump@jump_host" session_name@dest`
/// where dest = "{device}.{username}.{relay_host}"
#[allow(clippy::too_many_arguments)]
async fn launch_relay(
    relay_host: &str,
    jump_host: &str,
    relay_username: &str,
    relay_device: &str,
    session_name: &str,
    locale: Option<&str>,
    term: Option<&str>,
    extra_ssh_args: &[String],
) -> Result<MoshConnectInfo, MoshError> {
    // The virtual destination for the relay tunnel
    let dest = format!("{}.{}.{}", relay_device, relay_username, relay_host);

    // Resolve jump host IP for UDP fallback (same relay instance we tunnel through)
    let jump_host_address = resolve_first_ip(jump_host).await;

    // Build ProxyCommand that tunnels through the jump host.
    // This replicates the mobile app's: jumpClient.forwardLocal(dest, 22)
    let proxy_command = format!("ssh -W %h:%p jump@{}", jump_host);

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-o").arg(format!("ProxyCommand={}", proxy_command));

    for arg in extra_ssh_args {
        cmd.arg(arg);
    }

    cmd.arg("-tt"); // Force PTY allocation
    cmd.arg(format!("{}@{}", session_name, dest));
    cmd.arg(build_mosh_server_cmd(locale, term));

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    log::debug!(
        "launching mosh-server via relay SSH: jump={}, dest={}, session={}",
        jump_host,
        dest,
        session_name,
    );

    let output = run_ssh_command(&mut cmd).await?;
    let combined = format!("{}{}", output.0, output.1);
    let mut info = parse_mosh_output(&combined)?;
    info.jump_host_address = jump_host_address;
    Ok(info)
}

/// Build the mosh-server command string.
fn build_mosh_server_cmd(locale: Option<&str>, term: Option<&str>) -> String {
    let mut mosh_cmd = String::from("mosh-server new -s -c 256");
    if let Some(loc) = locale {
        mosh_cmd.push_str(&format!(" -l LANG={}", loc));
    }
    if let Some(t) = term {
        mosh_cmd.push_str(&format!(" -l TERM={}", t));
    }
    mosh_cmd
}

/// Run an SSH command and return (stdout, stderr).
async fn run_ssh_command(cmd: &mut tokio::process::Command) -> Result<(String, String), MoshError> {
    let output = cmd
        .output()
        .await
        .map_err(|e| MoshError::SshFailed(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(MoshError::SshFailed(format!(
            "SSH exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    Ok((stdout, stderr))
}

/// Resolve a hostname to its first IP address string.
async fn resolve_first_ip(host: &str) -> Option<String> {
    use tokio::net::lookup_host;
    let addr_str = format!("{}:22", host);
    let result = lookup_host(&addr_str).await;
    match result {
        Ok(mut addrs) => addrs.next().map(|a| a.ip().to_string()),
        Err(_) => None,
    }
}

/// Parse mosh-server stdout for connection info.
fn parse_mosh_output(output: &str) -> Result<MoshConnectInfo, MoshError> {
    let mut port = None;
    let mut key = None;
    let mut ip = None;

    for line in output.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("MOSH CONNECT ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                port = Some(
                    parts[0]
                        .parse::<u16>()
                        .map_err(|_| MoshError::InvalidMoshConnect)?,
                );
                key = Some(parts[1].to_string());
            }
        } else if let Some(rest) = line.strip_prefix("MOSH IP ") {
            ip = Some(rest.trim().to_string());
        }
    }

    match (port, key) {
        (Some(p), Some(k)) => Ok(MoshConnectInfo {
            port: p,
            key: k,
            ip,
            jump_host_address: None,
        }),
        _ => Err(MoshError::InvalidMoshConnect),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_output() {
        let output = "\n\nMOSH CONNECT 60001 AbCdEfGhIjKlMnOpQrStUw==\n\n";
        let info = parse_mosh_output(output).unwrap();
        assert_eq!(info.port, 60001);
        assert_eq!(info.key, "AbCdEfGhIjKlMnOpQrStUw==");
        assert!(info.ip.is_none());
    }

    #[test]
    fn parse_with_ip() {
        let output = "MOSH IP 192.168.1.100\nMOSH CONNECT 60002 TestKey12345678==\n";
        let info = parse_mosh_output(output).unwrap();
        assert_eq!(info.port, 60002);
        assert_eq!(info.key, "TestKey12345678==");
        assert_eq!(info.ip.as_deref(), Some("192.168.1.100"));
    }

    #[test]
    fn parse_missing_connect_fails() {
        let output = "some random output\n";
        assert!(parse_mosh_output(output).is_err());
    }

    #[test]
    fn build_mosh_cmd_full() {
        let cmd = build_mosh_server_cmd(Some("en_US.UTF-8"), Some("xterm-256color"));
        assert_eq!(
            cmd,
            "mosh-server new -s -c 256 -l LANG=en_US.UTF-8 -l TERM=xterm-256color"
        );
    }

    #[test]
    fn build_mosh_cmd_no_options() {
        let cmd = build_mosh_server_cmd(None, None);
        assert_eq!(cmd, "mosh-server new -s -c 256");
    }
}
