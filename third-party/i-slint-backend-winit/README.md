# Linux Hello compatibility patch for Slint’s winit backend

Based on published i-slint-backend-winit 1.18.1. Original source, copyright and
license notices are retained; Linux Hello uses the GPL-3.0-only option.
The original README is preserved in `README.upstream.txt`.

Changes: migrate desktop settings to zbus 6 (including standard D-Bus error
mapping), retain an executor independent of Tokio for Slint’s event loop, and
select crossfont for titled Wayland decorations. See the
[maintenance and provenance record](../README.md).
