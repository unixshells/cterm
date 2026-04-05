//! HTTP client for the Unix Shells API.

use super::types::*;

/// HTTP client for unixshells.com API.
pub struct UnixShellsClient {
    client: reqwest::Client,
    base_url: String,
}

impl UnixShellsClient {
    /// Create a new client pointing at unixshells.com.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            base_url: "https://unixshells.com".to_string(),
        }
    }

    /// Create a client pointing at a custom host (for testing).
    #[allow(dead_code)]
    pub fn with_host(host: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            base_url: format!("https://{}", host),
        }
    }

    /// Request to add this device to a user's account.
    ///
    /// Sends an approval email to the user. Returns the request ID for polling.
    pub async fn device_request(
        &self,
        username: &str,
        pubkey: &str,
        device: &str,
    ) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "username": username,
            "action": "add-key",
            "pubkey": pubkey,
            "device": device,
        });

        let resp = self
            .client
            .post(format!("{}/api/device-request", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Device request failed ({}): {}",
                status,
                text
            ));
        }

        let result: DeviceRequestResponse = resp.json().await?;
        Ok(result.id)
    }

    /// Poll the status of a device request.
    ///
    /// Returns the status. When approved, `username` field is populated.
    pub async fn poll_device_request(
        &self,
        request_id: &str,
    ) -> anyhow::Result<DeviceRequestStatus> {
        let resp = self
            .client
            .get(format!(
                "{}/api/device-request/{}",
                self.base_url, request_id
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Poll device request failed ({}): {}",
                status,
                text
            ));
        }

        let result: DeviceRequestStatus = resp.json().await?;
        Ok(result)
    }

    /// Get the list of devices and sessions for a user.
    ///
    /// Requires an auth token from `auth::sign_auth_token()`.
    pub async fn get_sessions(
        &self,
        username: &str,
        auth_token: &str,
    ) -> anyhow::Result<DeviceListResponse> {
        let resp = self
            .client
            .get(format!("{}/api/sessions/{}", self.base_url, username))
            .header("Authorization", format!("Bearer {}", auth_token))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Get sessions failed ({}): {}",
                status,
                text
            ));
        }

        let result: DeviceListResponse = resp.json().await?;
        Ok(result)
    }
}

impl Default for UnixShellsClient {
    fn default() -> Self {
        Self::new()
    }
}
