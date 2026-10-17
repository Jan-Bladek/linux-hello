//! Read-only fprintd diagnostics. Fingerprint templates remain owned by fprintd.
#![forbid(unsafe_code)]

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Reader {
    pub object_path: String,
    pub name: String,
    pub scan_type: String,
    pub enrolled_fingers: Vec<String>,
    pub enrollment_status: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("fprintd requires Linux")]
    UnsupportedPlatform,
    #[error("fprintd is unavailable or permission was denied")]
    Unavailable,
    #[error("fprintd request timed out")]
    Timeout,
    #[error("invalid fprintd response")]
    InvalidResponse,
}

#[cfg(target_os = "linux")]
#[zbus::proxy(
    interface = "net.reactivated.Fprint.Manager",
    default_service = "net.reactivated.Fprint",
    default_path = "/net/reactivated/Fprint/Manager"
)]
trait Manager {
    fn get_devices(&self) -> zbus::Result<Vec<zbus::OwnedObjectPath>>;
}

#[cfg(target_os = "linux")]
#[zbus::proxy(
    interface = "net.reactivated.Fprint.Device",
    default_service = "net.reactivated.Fprint"
)]
trait Device {
    #[zbus(property, name = "name")]
    fn name(&self) -> zbus::Result<String>;
    #[zbus(property, name = "scan-type")]
    fn scan_type(&self) -> zbus::Result<String>;
    fn list_enrolled_fingers(&self, username: &str) -> zbus::Result<Vec<String>>;
}

#[cfg(target_os = "linux")]
pub async fn readers() -> Result<Vec<Reader>, FingerprintError> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let connection = zbus::Connection::system()
            .await
            .map_err(|_| FingerprintError::Unavailable)?;
        query_readers(&connection).await
    })
    .await
    .map_err(|_| FingerprintError::Timeout)?
}

#[cfg(target_os = "linux")]
async fn query_readers(connection: &zbus::Connection) -> Result<Vec<Reader>, FingerprintError> {
    let manager = ManagerProxy::new(connection)
        .await
        .map_err(|_| FingerprintError::Unavailable)?;
    let paths = manager
        .get_devices()
        .await
        .map_err(|_| FingerprintError::Unavailable)?;
    if paths.len() > 32 {
        return Err(FingerprintError::InvalidResponse);
    }
    let mut readers = Vec::new();
    for path in paths {
        let device = DeviceProxy::builder(connection)
            .path(path.clone())
            .build()
            .await
            .map_err(|_| FingerprintError::Unavailable)?;
        let name = device
            .name()
            .await
            .map_err(|_| FingerprintError::Unavailable)?;
        let scan_type = device
            .scan_type()
            .await
            .map_err(|_| FingerprintError::Unavailable)?;
        // Empty username asks fprintd to resolve the authenticated bus caller.
        let (enrolled_fingers, enrollment_status) = match device.list_enrolled_fingers("").await {
            Ok(fingers) if fingers.len() <= 10 => (fingers, "available".into()),
            Ok(_) => return Err(FingerprintError::InvalidResponse),
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "net.reactivated.Fprint.Error.NoEnrolledPrints" =>
            {
                (Vec::new(), "not enrolled".into())
            }
            Err(_) => (Vec::new(), "unknown: unavailable or access denied".into()),
        };
        readers.push(Reader {
            object_path: path.to_string(),
            name,
            scan_type,
            enrolled_fingers,
            enrollment_status,
        });
    }
    Ok(readers)
}

#[cfg(not(target_os = "linux"))]
pub async fn readers() -> Result<Vec<Reader>, FingerprintError> {
    Err(FingerprintError::UnsupportedPlatform)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    struct SimulatedManager;
    struct SimulatedReader;

    #[zbus::interface(name = "net.reactivated.Fprint.Manager")]
    impl SimulatedManager {
        fn get_devices(&self) -> Vec<zbus::OwnedObjectPath> {
            vec!["/net/reactivated/Fprint/Device/0".try_into().unwrap()]
        }
    }

    #[zbus::interface(name = "net.reactivated.Fprint.Device")]
    impl SimulatedReader {
        #[zbus(property, name = "name")]
        fn name(&self) -> &str {
            "Simulated test reader"
        }
        #[zbus(property, name = "scan-type")]
        fn scan_type(&self) -> &str {
            "press"
        }
        fn list_enrolled_fingers(&self, username: &str) -> zbus::fdo::Result<Vec<String>> {
            if !username.is_empty() {
                return Err(zbus::fdo::Error::AccessDenied(
                    "client must use its own caller context".into(),
                ));
            }
            Ok(vec!["right-index-finger".into()])
        }
    }

    #[tokio::test]
    #[ignore = "requires isolated dbus-run-session; see tests/integration/run-session.sh"]
    async fn isolated_fprintd_protocol_uses_caller_identity() {
        let _server = zbus::connection::Builder::session()
            .name("net.reactivated.Fprint")
            .serve_at("/net/reactivated/Fprint/Manager", SimulatedManager)
            .serve_at("/net/reactivated/Fprint/Device/0", SimulatedReader)
            .build()
            .await
            .unwrap();
        let client = zbus::Connection::session().await.unwrap();
        let readers = super::query_readers(&client).await.unwrap();
        assert_eq!(readers.len(), 1);
        assert_eq!(readers[0].name, "Simulated test reader");
        assert_eq!(readers[0].enrolled_fingers, ["right-index-finger"]);
        assert_eq!(readers[0].enrollment_status, "available");
    }
}
