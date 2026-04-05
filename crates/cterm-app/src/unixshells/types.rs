//! Data types for Unix Shells integration.

use serde::{Deserialize, Serialize};

/// State of the Unix Shells login.
#[derive(Debug, Clone)]
pub enum LoginState {
    /// Not logged in.
    LoggedOut,
    /// Waiting for user to approve device via email.
    PendingApproval {
        request_id: String,
        username: String,
    },
    /// Logged in and ready to browse devices.
    LoggedIn { username: String },
}

/// A remote device discovered via the Unix Shells API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub sessions: Vec<SessionInfo>,
}

/// A terminal session running on a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub title: String,
}

/// Response from GET /api/sessions/{username}.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceListResponse {
    #[serde(default)]
    pub devices: Vec<DeviceInfo>,
}

/// Response from POST /api/device-request.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceRequestResponse {
    pub id: String,
}

/// Response from GET /api/device-request/{id}.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceRequestStatus {
    pub status: String,
    #[serde(default)]
    pub username: Option<String>,
}
