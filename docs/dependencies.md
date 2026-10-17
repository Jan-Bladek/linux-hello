# Dependency status and release blockers

Exact direct version pins and API references are listed in the
[research version baseline](research.md#version-baseline) and root `Cargo.toml`.
`rust-toolchain.toml` pins Rust 1.99.0; `Cargo.lock` records the resolved versions
and registry checksums. Build and test with `--locked`.

Audit snapshot: 2026-10-10, repeated on native Fedora 2026-10-11 with the same
result; `cargo-audit` 0.22.2, committed lockfile, RustSec
database commit `7eebec69c352c7191b1f13eb95dd510eeca5d1de` (updated 2026-10-09).
The run checked 600 lockfile dependencies and reported **zero known vulnerability
entries and two maintenance advisories**. This is not a clean strict audit or a
guarantee that there are no vulnerabilities.

| Locked dependency | Advisory | Consequence at the recorded audit |
|---|---|---|
| `bincode` 2.0.1 | [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | Unmaintained dependency in the Slint graph; no patched version listed |
| `ttf-parser` 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | Unmaintained font-parsing dependency in the GUI/windowing graph; no patched version listed |

These are informational maintenance advisories, not reports of an identified
exploit in Linux Hello. They still matter for a security-oriented project and are
recorded as release blockers. `cargo audit --deny warnings` is intentionally strict
and failed in the recorded audit; release qualification remains blocked until the dependency graph is remediated or a separately
reviewed, narrowly scoped policy decision is made. No ignore rules are added to
make it pass.

Before an authentication release, update the affected upstream graph to maintained
replacements, or review a minimal maintained fork with pinned provenance,
compatible licensing and a documented maintenance owner. Do not force a
semver-incompatible patch solely to remove an advisory. Re-run build, tests,
dependency/license checks and desktop validation after the change. A normal
non-strict `cargo audit` exit status must not be reported as a passing strict gate.

The source-delivery check also runs `cargo-deny` 0.20.2. License/source checks
passed for the captured graph. Local workspace paths have explicit version
requirements so the wildcard ban can remain enabled. Duplicate dependency versions
are reported as warnings: review them on upgrades, but they are not necessarily
vulnerabilities. The SPDX allowlist does not waive source/notice obligations or
certify Fedora packaging compliance.

Reproduce the dependency checks from the repository root:

```sh
cargo audit --deny warnings
cargo deny --locked check licenses sources bans
cargo tree --locked -i bincode
cargo tree --locked -i ttf-parser
```

The shared code's Apache-2.0 license and the GUI's GPL-3.0-only license are
intentional. No face model, ONNX Runtime binary or model-training dataset is
bundled. Any future model/runtime addition requires separate provenance, license,
telemetry and resource-isolation review; see [research](research.md).

## Optional OpenCV adapter (2026-10-11)

The exact optional Rust binding is `opencv` 0.101.1 (MIT), with default features
disabled and `clang-runtime`, `dnn`, `objdetect`, and `imgproc` enabled. The native
Fedora library tested was OpenCV 4.13.0. Its system-library dependencies are not
covered by Cargo audit. Model pins, upstream notices and unresolved dataset
provenance are recorded in [inference diagnostics](inference.md). The lockfile
has changed; the 600-dependency audit count above describes the previous graph.

The expanded 609-dependency lockfile was audited on 2026-10-11: zero known
vulnerability entries and the same two unmaintained advisories. Strict audit failed in this historical snapshot. The 2026-10-14 technical audit
found a missing version on hello-sudo → hello-cli that made the bans check fail;
the audit remediation adds that constraint, and a new local license/source/bans
run passes with warnings.
No suppressions were introduced.

The remediation run on 2026-10-14 checks 615 lockfile entries: strict advisories
still fail with the same two unmaintained dependencies and zero reported known
vulnerability entries. No warning exclusions were introduced. These counts
describe this run, not a permanent guarantee. Use `cargo tree --workspace
--all-features --target all -i ttf-parser` to include the GUI in reachability checks.

## Initial follow-up graph — 2026-10-17 (historical)

First-party IPC/daemon/fprintd code now uses pinned zbus 6.0.0 and its integrated
wire/name types, with explicit Tokio/proxy/service/object-manager features.
Tokio is pinned to 1.53.3 to meet its minimum patch requirement. Fallible builder
configuration now reports errors from `build()` per the new API; no caller-supplied
UID or biometric RPC was added. See [upstream zbus 6 documentation](https://docs.rs/zbus/6.0.0/zbus/).

At that commit, this **did not clear the strict advisory gate**. Slint 1.18.1's winit backend
still brings zbus 5.19.0 / zvariant 5.15.0 / zbus_names 4.3.4 and the old derives.
The Linux GUI font chain retains ttf-parser 0.25.1, while bincode 2.0.1 remains in
the cross-platform lockfile. That strict run reported five unmaintained
warnings and zero known vulnerability entries, not an authentication exploit or
clean bill of health. No ignores, checksums or lock entries were manually removed.
Maintained GUI/windowing upstream replacements and native-library auditing remain
open. The [follow-up tracker](audit-remediation-2026-10-17.md) records the scope.


## Strict advisory remediation — 2026-10-17

The subsequent GUI dependency fix removes all five flagged packages from the
resolved lockfile. `cargo audit --deny warnings` passes against the same 1,338
RustSec advisories with 602 dependencies. No advisory exclusions or CI policy
relaxations were added. This is a dated dependency result, not a biometric or
native-library security certification.

Slint 1.18.1 is retained. Its narrowly patched winit backend now uses zbus 6 for
portal settings, and crossfont for titled Wayland decorations. A small SCTK
Adwaita compatibility patch selects crossfont 0.9 / fontconfig-sys 6. The build
pins Slint's compatible typed-index-collections 3.2.0 helper to avoid an unused
optional bincode dependency introduced in subsequent 3.x releases. Existing
Fontconfig/FreeType development packages satisfy the native build requirements.

The [patch maintenance record](../third-party/README.md) includes exact upstream
archive checksums, commits, licensing, changes and retirement instructions.
Both patched source directories are required by installers and RPM source
archives; normal `cargo vendor` covers the remaining registry dependencies.

No biometric format, model, threshold, enrollment, PAM interface or desktop
activation configuration changes. Re-enrollment is not required. The regression
suite exercises the actual portal transport source against modern and legacy
mock portals on a private session bus, without a Tokio runtime.
