# Linux Hello compatibility patch for SCTK Adwaita

Based on published sctk-adwaita 0.10.1 (MIT). Original source and notices are
retained. The original README and changelog are preserved as `.upstream.txt`.

Changes: use crossfont 0.9.0 to share fontconfig-sys 6 with Slint, and make six
elided lifetimes explicit for current Rust warning checks. Crossfont 0.9 keeps
the renderer API; its breaking change is the fontconfig-sys version. See the
[maintenance and provenance record](../README.md).
