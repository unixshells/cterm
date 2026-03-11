//! Platform-specific socket path management

use std::path::PathBuf;

/// Get the default socket path for ctermd.
///
/// On macOS, returns a Unix socket path under `~/Library/Application Support/com.cterm.terminal/`.
/// On Linux, returns a Unix socket path under `$XDG_RUNTIME_DIR/cterm/` or `/tmp/`.
/// On Windows, returns a named pipe path like `\\.\pipe\ctermd-{USERNAME}`.
pub fn default_socket_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let mut path = PathBuf::from(home);
            path.push("Library/Application Support/com.cterm.terminal");
            std::fs::create_dir_all(&path).ok();
            path.push("ctermd.sock");
            return path;
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Prefer XDG_RUNTIME_DIR (per-user, tmpfs)
        if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let mut path = PathBuf::from(runtime_dir);
            path.push("cterm");
            std::fs::create_dir_all(&path).ok();
            path.push("ctermd.sock");
            return path;
        }
    }

    // Fallback: /tmp/ctermd-{uid}.sock
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/ctermd-{}.sock", uid))
    }

    #[cfg(windows)]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        PathBuf::from(format!(r"\\.\pipe\ctermd-{}", username))
    }
}

/// Get the path where the ctermd PID file is stored
pub fn pid_file_path() -> PathBuf {
    let mut path = default_socket_path();
    path.set_extension("pid");
    path
}
