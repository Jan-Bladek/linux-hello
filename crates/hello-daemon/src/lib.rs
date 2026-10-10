//! Diagnostic service only: no camera acquisition, templates, or authentication.
#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
mod linux {
    use hello_ipc::{BUS_NAME, OBJECT_PATH};
    use zbus::{Connection, fdo, message::Header};

    pub struct Diagnostics;

    #[zbus::interface(name = "org.linuxhello.Daemon1")]
    impl Diagnostics {
        fn get_version(&self) -> &str {
            env!("CARGO_PKG_VERSION")
        }

        fn get_capabilities(&self) -> fdo::Result<String> {
            serde_json::to_string(&hello_ipc::Capabilities::default())
                .map_err(|_| fdo::Error::Failed("unable to encode capabilities".into()))
        }

        fn get_security_status(&self) -> fdo::Result<String> {
            serde_json::to_string(&hello_core::SecurityStatus::default())
                .map_err(|_| fdo::Error::Failed("unable to encode security status".into()))
        }

        async fn get_requester_uid(
            &self,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] connection: &Connection,
        ) -> fdo::Result<u32> {
            let sender = header.sender().ok_or_else(|| {
                fdo::Error::AccessDenied("authenticated bus sender required".into())
            })?;
            let bus = zbus::fdo::DBusProxy::new(connection).await?;
            // Resolve credentials using the bus-assigned unique name, never a
            // method argument or client-provided user/session/process identity.
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                bus.get_connection_unix_user(sender.clone().into()),
            )
            .await
            .map_err(|_| fdo::Error::Timeout("credential lookup timed out".into()))?
        }
    }

    pub async fn serve(session_bus: bool) -> zbus::Result<Connection> {
        let builder = if session_bus {
            zbus::connection::Builder::session()?
        } else {
            zbus::connection::Builder::system()?
        };
        builder
            .name(BUS_NAME)?
            .serve_at(OBJECT_PATH, Diagnostics)?
            .build()
            .await
    }

    #[cfg(test)]
    mod tests {
        #[tokio::test]
        #[ignore = "requires isolated dbus-run-session; see tests/integration/run-session.sh"]
        async fn isolated_bus_identity_concurrency_and_denial() {
            let _server = super::serve(true).await.unwrap();
            let connection = zbus::Connection::session().await.unwrap();
            let expected_uid = zbus::fdo::DBusProxy::new(&connection)
                .await
                .unwrap()
                .get_connection_unix_user(connection.unique_name().unwrap().clone().into())
                .await
                .unwrap();
            let mut requests = tokio::task::JoinSet::new();
            for _ in 0..16 {
                requests.spawn(hello_ipc::status(true));
            }
            while let Some(result) = requests.join_next().await {
                let status = result.unwrap().unwrap();
                assert_eq!(status.caller_uid, expected_uid);
                assert!(!status.security.authentication_ready());
            }
            let proxy = zbus::Proxy::new(
                &connection,
                hello_ipc::BUS_NAME,
                hello_ipc::OBJECT_PATH,
                hello_ipc::INTERFACE,
            )
            .await
            .unwrap();
            let response: zbus::Result<()> = proxy
                .call("StartVerification", &(expected_uid, "replay-token"))
                .await;
            assert!(
                matches!(response, Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod")
            );
            // Credential method accepts no user identifier; supplying one is invalid.
            let response: zbus::Result<u32> = proxy.call("GetRequesterUid", &(0u32,)).await;
            assert!(response.is_err());
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::serve;
