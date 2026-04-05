//! Unix Shells (unixshells.com) integration.
//!
//! Provides login, device discovery, and connection management for
//! the Unix Shells relay service.

pub mod api_client;
pub mod auth;
pub mod device_service;
pub mod types;

pub use device_service::DeviceService;
pub use types::{DeviceInfo, LoginState, SessionInfo};
