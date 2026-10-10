# Dependency status and release blockers

Exact direct version pins and API references are listed in the
[research version baseline](research.md#version-baseline) and root `Cargo.toml`.
`rust-toolchain.toml` pins Rust 1.99.0; `Cargo.lock` records the resolved versions
and registry checksums. Build and test with `--locked`.

Audit snapshot: 2026-10-10, `cargo-audit` 0.22.2, committed lockfile, RustSec
database commit `7eebec69c352c7191b1f13eb95dd510eeca5d1de` (updated 2026-10-09).
The run checked 600 lockfile dependencies and reported **zero known vulnerability
entries and two maintenance advisories**. This is not a clean strict audit or a
guarantee that there are no vulnerabilities.

| Locked dependency | Advisory | Current consequence |
|---|---|---|
| `bincode` 2.0.1 | [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | Unmaintained dependency in the Slint graph; no patched version listed |
| `ttf-parser` 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | Unmaintained font-parsing dependency in the GUI/windowing graph; no patched version listed |

These are informational maintenance advisories, not reports of an identified
exploit in Linux Hello. They still matter for a security-oriented project and are
recorded as release blockers. `cargo audit --deny warnings` is intentionally strict
and currently fails until the dependency graph is remediated or a separately
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
