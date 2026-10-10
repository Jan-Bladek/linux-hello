//! Versioned, bounded diagnostic IPC. This interface cannot authorize a login.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const BUS_NAME: &str = "org.linuxhello.Daemon1";
pub const OBJECT_PATH: &str = "/org/linuxhello/Daemon1";
pub const INTERFACE: &str = "org.linuxhello.Daemon1";
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub protocol_version: u32,
    pub service_version: String,
    pub diagnostic_only: bool,
    pub face_enrollment: bool,
    pub face_authentication: bool,
    pub pam_module: bool,
    pub camera_transport: String,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            service_version: env!("CARGO_PKG_VERSION").into(),
            diagnostic_only: true,
            face_enrollment: false,
            face_authentication: false,
            pam_module: false,
            camera_transport: "direct local V4L2; no frames on D-Bus".into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("D-Bus diagnostics require Linux")]
    UnsupportedPlatform,
    #[error("daemon unavailable, incompatible, or access denied")]
    Unavailable,
    #[error("daemon request timed out")]
    Timeout,
    #[error("invalid or oversized daemon response")]
    InvalidResponse,
}

pub fn decode_bounded<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, IpcError> {
    if json.len() > MAX_RESPONSE_BYTES {
        return Err(IpcError::InvalidResponse);
    }
    serde_json::from_str(json).map_err(|_| IpcError::InvalidResponse)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticStatus {
    pub capabilities: Capabilities,
    pub security: hello_core::SecurityStatus,
    pub caller_uid: u32,
    /// A session bus service has no authority over host authentication.
    pub session_bus: bool,
}

#[cfg(target_os = "linux")]
#[zbus::proxy(
    interface = "org.linuxhello.Daemon1",
    default_service = "org.linuxhello.Daemon1",
    default_path = "/org/linuxhello/Daemon1"
)]
pub trait Daemon {
    fn get_version(&self) -> zbus::Result<String>;
    fn get_capabilities(&self) -> zbus::Result<String>;
    fn get_security_status(&self) -> zbus::Result<String>;
    fn get_requester_uid(&self) -> zbus::Result<u32>;
}

#[cfg(target_os = "linux")]
pub async fn status(session_bus: bool) -> Result<DiagnosticStatus, IpcError> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let connection = if session_bus {
            zbus::Connection::session().await
        } else {
            zbus::Connection::system().await
        }
        .map_err(|_| IpcError::Unavailable)?;
        let proxy = DaemonProxy::new(&connection)
            .await
            .map_err(|_| IpcError::Unavailable)?;
        let capabilities: Capabilities = decode_bounded(
            &proxy
                .get_capabilities()
                .await
                .map_err(|_| IpcError::Unavailable)?,
        )?;
        if capabilities.protocol_version != 1
            || !capabilities.diagnostic_only
            || capabilities.face_authentication
            || capabilities.pam_module
            || capabilities.face_enrollment
        {
            return Err(IpcError::InvalidResponse);
        }
        let security = decode_bounded(
            &proxy
                .get_security_status()
                .await
                .map_err(|_| IpcError::Unavailable)?,
        )?;
        let caller_uid = proxy
            .get_requester_uid()
            .await
            .map_err(|_| IpcError::Unavailable)?;
        Ok(DiagnosticStatus {
            capabilities,
            security,
            caller_uid,
            session_bus,
        })
    })
    .await
    .map_err(|_| IpcError::Timeout)?
}

#[cfg(not(target_os = "linux"))]
pub async fn status(_session_bus: bool) -> Result<DiagnosticStatus, IpcError> {
    Err(IpcError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_never_advertise_unbuilt_security() {
        let caps = Capabilities::default();
        assert!(caps.diagnostic_only);
        assert!(!caps.face_authentication && !caps.face_enrollment && !caps.pam_module);
    }

    #[test]
    fn malformed_oversized_and_unknown_responses_rejected() {
        assert!(decode_bounded::<Capabilities>("{}").is_err());
        assert!(decode_bounded::<Capabilities>(&" ".repeat(MAX_RESPONSE_BYTES + 1)).is_err());
        let mut value = serde_json::to_value(Capabilities::default()).unwrap();
        value["token"] = "untrusted".into();
        assert!(decode_bounded::<Capabilities>(&value.to_string()).is_err());
    }
}
