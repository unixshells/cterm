//! Background service for Unix Shells login and device discovery.

use super::api_client::UnixShellsClient;
use super::auth;
use super::types::*;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Background service that manages Unix Shells login state and device polling.
pub struct DeviceService {
    state: Arc<RwLock<ServiceState>>,
    /// Monotonically increasing version — UI polls this to detect changes.
    pub version: Arc<AtomicU64>,
    config_dir: PathBuf,
    client: Arc<UnixShellsClient>,
}

struct ServiceState {
    login_state: LoginState,
    devices: Vec<DeviceInfo>,
    last_error: Option<String>,
    /// Handle to cancel polling tasks.
    poll_cancel: Option<tokio::sync::watch::Sender<bool>>,
}

impl DeviceService {
    /// Create a new device service.
    ///
    /// If `username` is provided (from saved config), starts in LoggedIn state
    /// and begins device polling immediately.
    pub fn new(config_dir: PathBuf, username: Option<String>) -> Self {
        let login_state = match username {
            Some(ref u) if !u.is_empty() => LoginState::LoggedIn {
                username: u.clone(),
            },
            _ => LoginState::LoggedOut,
        };

        let service = Self {
            state: Arc::new(RwLock::new(ServiceState {
                login_state,
                devices: Vec::new(),
                last_error: None,
                poll_cancel: None,
            })),
            version: Arc::new(AtomicU64::new(0)),
            config_dir,
            client: Arc::new(UnixShellsClient::new()),
        };

        // If already logged in, start polling
        if username.is_some_and(|u| !u.is_empty()) {
            service.start_device_polling();
        }

        service
    }

    /// Get the current login state.
    pub fn login_state(&self) -> LoginState {
        self.state.read().login_state.clone()
    }

    /// Get the current device list.
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.state.read().devices.clone()
    }

    /// Get the last error message (if any).
    pub fn last_error(&self) -> Option<String> {
        self.state.read().last_error.clone()
    }

    /// Start the login flow.
    ///
    /// Generates/loads the Ed25519 key, sends a device-request to the API,
    /// and begins polling for email approval.
    /// Returns the request_id on success.
    pub fn start_login(&self, username: &str) -> anyhow::Result<String> {
        let key = auth::load_or_generate_relay_key(&self.config_dir)?;
        let pubkey = auth::public_key_openssh(&key)?;
        let device = auth::device_name();
        let username = username.to_string();

        let client = self.client.clone();
        let state = self.state.clone();
        let version = self.version.clone();
        let config_dir = self.config_dir.clone();

        // Send device request synchronously from a blocking context
        // (The caller should be on a background thread)
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let request_id =
            rt.block_on(async { client.device_request(&username, &pubkey, &device).await })?;

        // Update state to pending
        {
            let mut s = state.write();
            s.login_state = LoginState::PendingApproval {
                request_id: request_id.clone(),
                username: username.clone(),
            };
            s.last_error = None;
        }
        version.fetch_add(1, Ordering::Relaxed);

        // Start polling for approval in background
        let rid = request_id.clone();
        let client2 = self.client.clone();
        let state2 = self.state.clone();
        let version2 = self.version.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                poll_for_approval(client2, state2, version2, &rid, &username, &config_dir).await;
            });
        });

        Ok(request_id)
    }

    /// Sign out and stop polling.
    pub fn sign_out(&self) {
        let mut s = self.state.write();
        // Cancel polling
        if let Some(cancel) = s.poll_cancel.take() {
            let _ = cancel.send(true);
        }
        s.login_state = LoginState::LoggedOut;
        s.devices.clear();
        s.last_error = None;
        self.version.fetch_add(1, Ordering::Relaxed);

        // Clear relay_username from config
        if let Ok(mut config) = crate::load_config() {
            config.latch.relay_username = None;
            let _ = crate::config::save_config(&config);
        }
    }

    /// Start background device polling (called after login or on startup).
    fn start_device_polling(&self) {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Store cancel handle
        self.state.write().poll_cancel = Some(cancel_tx);

        let state = self.state.clone();
        let version = self.version.clone();
        let client = self.client.clone();
        let config_dir = self.config_dir.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                device_poll_loop(client, state, version, config_dir, cancel_rx).await;
            });
        });
    }
}

/// Poll for device-request approval.
async fn poll_for_approval(
    client: Arc<UnixShellsClient>,
    state: Arc<RwLock<ServiceState>>,
    version: Arc<AtomicU64>,
    request_id: &str,
    username: &str,
    config_dir: &std::path::Path,
) {
    for _ in 0..300 {
        // ~15 minutes at 3s intervals
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Check if we've been cancelled (sign out)
        {
            let s = state.read();
            if matches!(s.login_state, LoginState::LoggedOut) {
                return;
            }
        }

        match client.poll_device_request(request_id).await {
            Ok(status) if status.status == "approved" => {
                log::info!("Device approved for user '{}'", username);

                // Save username to config
                if let Ok(mut config) = crate::load_config() {
                    config.latch.relay_username = Some(username.to_string());
                    let _ = crate::config::save_config(&config);
                }

                // Transition to logged in
                {
                    let mut s = state.write();
                    s.login_state = LoginState::LoggedIn {
                        username: username.to_string(),
                    };
                    s.last_error = None;
                }
                version.fetch_add(1, Ordering::Relaxed);

                // Start device polling
                let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                state.write().poll_cancel = Some(cancel_tx);

                let client2 = client.clone();
                let state2 = state.clone();
                let version2 = version.clone();
                let config_dir2 = config_dir.to_path_buf();
                tokio::spawn(async move {
                    device_poll_loop(client2, state2, version2, config_dir2, cancel_rx).await;
                });

                return;
            }
            Ok(_) => {
                // Still pending
            }
            Err(e) => {
                log::warn!("Poll error: {}", e);
                state.write().last_error = Some(e.to_string());
                version.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // Timed out
    log::warn!("Device approval timed out");
    let mut s = state.write();
    s.login_state = LoginState::LoggedOut;
    s.last_error = Some("Approval timed out. Please try again.".to_string());
    version.fetch_add(1, Ordering::Relaxed);
}

/// Background loop that polls for device sessions.
async fn device_poll_loop(
    client: Arc<UnixShellsClient>,
    state: Arc<RwLock<ServiceState>>,
    version: Arc<AtomicU64>,
    config_dir: PathBuf,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = cancel_rx.changed() => {
                log::info!("Device polling cancelled");
                return;
            }
        }

        // Get username from state
        let username = {
            let s = state.read();
            match &s.login_state {
                LoginState::LoggedIn { username } => username.clone(),
                _ => return,
            }
        };

        // Sign auth token
        let key = match auth::load_or_generate_relay_key(&config_dir) {
            Ok(k) => k,
            Err(e) => {
                log::error!("Failed to load relay key: {}", e);
                continue;
            }
        };
        let token = match auth::sign_auth_token(&key) {
            Ok(t) => t,
            Err(e) => {
                log::error!("Failed to sign auth token: {}", e);
                continue;
            }
        };

        match client.get_sessions(&username, &token).await {
            Ok(resp) => {
                let mut s = state.write();
                s.devices = resp.devices;
                s.last_error = None;
                version.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                log::warn!("Failed to fetch devices: {}", e);
                let mut s = state.write();
                s.last_error = Some(e.to_string());
                version.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
