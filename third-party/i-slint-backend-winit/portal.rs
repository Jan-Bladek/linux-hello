// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// Extracted from xdg_desktop_settings.rs for direct regression tests in hello-settings.

/// Reads one setting.
/// Portals older than 1.15 (Ubuntu 22.04 and earlier) only implement the deprecated `Read`,
/// whose extra variant wrapping `downcast_ref()` unwraps transparently.
pub(super) async fn read_setting(
    settings_proxy: &zbus::Proxy<'_>,
    namespace: &str,
    key: &str,
) -> zbus::Result<zbus::OwnedVariant> {
    let args = (namespace, key);
    match settings_proxy.call("ReadOne", &args).await {
        Err(zbus::Error::FDO(err)) if matches!(*err, zbus::fdo::Error::UnknownMethod(_)) => {
            settings_proxy.call("Read", &args).await
        }
        result => result,
    }
}

/// True when the error only means the session bus or the settings portal is missing,
/// which is normal on headless systems and bare compositors.
pub(super) fn portal_unavailable(err: &zbus::Error) -> bool {
    match err {
        // No session bus address, or nothing listening on it.
        zbus::Error::Address(_) | zbus::Error::InputOutput(_) | zbus::Error::Connection(..) => true,
        zbus::Error::MethodError(name, ..) => matches!(
            name.as_str(),
            "org.freedesktop.DBus.Error.ServiceUnknown"
                | "org.freedesktop.DBus.Error.NameHasNoOwner"
        ),
        zbus::Error::FDO(err) => matches!(
            **err,
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LegacySettings;
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl LegacySettings {
        fn read(&self, namespace: &str, key: &str) -> zbus::OwnedVariant {
            assert_eq!(
                (namespace, key),
                ("org.freedesktop.appearance", "color-scheme")
            );
            zbus::Variant::new(zbus::Variant::from(1u32))
                .try_into()
                .unwrap()
        }
    }
    struct ModernSettings;
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl ModernSettings {
        fn read_one(&self, namespace: &str, key: &str) -> zbus::OwnedVariant {
            assert_eq!(
                (namespace, key),
                ("org.freedesktop.appearance", "color-scheme")
            );
            2u32.into()
        }
    }
    #[test]
    fn portal_absence_and_permission_denial_are_distinct() {
        assert!(portal_unavailable(&zbus::Error::FDO(Box::new(
            zbus::fdo::Error::ServiceUnknown("absent".into())
        ))));
        assert!(!portal_unavailable(&zbus::Error::FDO(Box::new(
            zbus::fdo::Error::AccessDenied("denied".into())
        ))));
    }
    // No Tokio runtime: Slint polls this transport from its own local event loop.
    #[test]
    #[ignore = "requires dbus-run-session; see tests/integration/run-session.sh"]
    fn portal_bus_modern_and_legacy_reads_without_tokio() {
        futures::executor::block_on(async {
            let _legacy = zbus::connection::Builder::session()
                .name("org.linuxhello.TestLegacySettings")
                .serve_at("/org/freedesktop/portal/desktop", LegacySettings)
                .build()
                .await
                .unwrap();
            let _modern = zbus::connection::Builder::session()
                .name("org.linuxhello.TestModernSettings")
                .serve_at("/org/freedesktop/portal/desktop", ModernSettings)
                .build()
                .await
                .unwrap();
            let client = zbus::Connection::session().await.unwrap();
            for (destination, expected) in [
                ("org.linuxhello.TestLegacySettings", 1u32),
                ("org.linuxhello.TestModernSettings", 2u32),
            ] {
                let proxy = zbus::proxy::Builder::new(&client)
                    .destination(destination)
                    .path("/org/freedesktop/portal/desktop")
                    .interface("org.freedesktop.portal.Settings")
                    .build()
                    .await
                    .unwrap();
                let value = read_setting(&proxy, "org.freedesktop.appearance", "color-scheme")
                    .await
                    .unwrap();
                assert_eq!(value.downcast_ref::<u32>().unwrap(), expected);
            }
        });
    }
}
