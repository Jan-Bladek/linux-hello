# Reviewed GUI dependency compatibility patches

Linux Hello maintainers own these narrow local patches until compatible upstream
releases replace them. They are source patches, not renamed copies of retired
crates. They preserve the existing Slint 1.18.1 UI, software renderer, X11 and
Wayland backends. The root Cargo patch table selects them in online, offline,
installer and RPM source builds. Do not delete this directory when vendoring.

## Provenance

Sources are the published crates.io archives, with original copyright and license
notices retained. `Cargo.toml.orig` and `.upstream.txt` files preserve upstream
metadata/documentation; builds use the patched normalized `Cargo.toml` files.

| Crate | Original archive SHA-256 | Upstream Git commit |
| --- | --- | --- |
| i-slint-backend-winit 1.18.1 | `dae4fcf1b19e9cb109984d2af87d2b9485388c619265bff71dd523133888bee2` | `372cf0ee5577c3dfec309a45e7b778ba4e81b734` (internal/backends/winit) |
| sctk-adwaita 0.10.1 | `b6277f0217056f77f1d8f49f2950ac6c278c0d607c45f5ee99328d792ede24ec` | `d5f343c45c04d4a026db57f76760819b7325db00` |

Upstreams: [Slint](https://github.com/slint-ui/slint) and
[SCTK Adwaita](https://github.com/PolyMeilex/sctk-adwaita).
The backend retains its GPL-3.0-only licensing route; SCTK Adwaita is MIT.
The unchanged, unused upstream Cantarell font asset remains under OFL-1.1,
with its embedded copyright and a separate `Cantarell-OFL.txt` notice. The selected
crossfont renderer uses system fonts, not that asset. Upstream CI metadata is not
vendored. No biometric model or authentication implementation is forked here.

## Reviewed changes

- Backend desktop settings: zbus 6.0.0 with integrated `OwnedVariant`, fallible
  builder validation at `build()`, and standard `UnknownMethod` as an FDO error.
  Retain fallback from `ReadOne` to legacy `Read`, including nested variants.
  Keep `default-rt` so Slint's local event loop does not require entering Tokio.
  `portal.rs` extracts the existing transport helpers, tested directly by the GUI
  test target using the same source file, without copying the implementation.
- Wayland title text: choose winit's existing crossfont decoration feature instead
  of ab_glyph/owned_ttf_parser/ttf-parser. Titles and decorations remain enabled.
- SCTK Adwaita: crossfont 0.8 → pinned 0.9.0, whose API is unchanged for this
  renderer and which shares fontconfig-sys 6 with Slint. Six elided lifetimes are
  explicit for current Rust; the renderer's layout and drawing are unchanged.
- Upstream READMEs/changelog are retained as text so repository documentation
  checks do not interpret Rustdoc-only links as filesystem paths. Slint's crate
  documentation still includes the original README.

The GUI build also pins the compatible Slint compiler helper
`typed-index-collections = 3.2.0`, a compatible 3.x release without the optional retired
bincode 2 codec. Slint requires ^3.2; its collections API is unchanged for this
application. Linux Hello never enables that codec or serializes templates with
it. This is a temporary resolver constraint, not a claim that all old releases
are preferable. Reassess it when Slint supports the maintained 4.x API.

## Validation and retirement

Run the root workspace tests, optional feature Clippy/tests, isolated session-bus
suite (modern and legacy portals outside Tokio), strict Cargo audit and
license/source/bans policy. Render the GUI and smoke-test the native Wayland
backend after font changes. Fontconfig/FreeType development packages were already
required by the installer and CI; no new distro package is required.

On each Slint upgrade, diff both published archives against these sources,
review upstream security/API changes, rebase or remove the patches and rerun the
checks. Do not bump the backend independently of Slint. Keep strict advisories
and the empty advisory ignore list; no exception is needed for this graph.
Native Fontconfig/FreeType security remains the distribution's responsibility.
