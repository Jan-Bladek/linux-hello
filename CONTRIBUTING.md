# Contributing

Start with [architecture](docs/architecture.md), the
[threat model](docs/threat-model.md), [research](docs/research.md) and the
[installation guide](docs/installation.md). This workspace is a development preview;
contributors must distinguish implemented behavior, simulated tests and physical
hardware evidence.

Use the pinned toolchain in `rust-toolchain.toml`. On Linux install the build
libraries in the installation guide, then run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash tests/integration/run-session.sh
cargo build --workspace --release --locked
cargo audit --deny warnings
cargo deny --locked check licenses sources bans
```

CI pins `cargo-audit` 0.22.2 and `cargo-deny` 0.20.2. A camera and desktop compositor
are not required for the ordinary test suite. Hardware tests must be explicitly
selected, consented and labeled. Never put personal biometric fixtures in Git.
The strict advisory check currently fails on the documented upstream maintenance
advisories in [dependency status](docs/dependencies.md); do not suppress them.

Keep OS-specific code out of portable biometric/validation modules. Prefer typed,
bounded inputs, explicit errors and RAII resource cleanup. `unsafe` code needs a
local safety explanation and a testable boundary. Do not add subprocesses or shell
commands to an authentication path. Use property/fuzz testing for untrusted parsers
when introducing new formats.

For a security-sensitive change, describe the attacker, trust boundary, failure
behavior, authorization binding and negative tests. Include disconnection,
cancellation, timeout, stale results and concurrency where applicable. A UI toggle
must not imply a security property that the backend cannot establish.

Dependency updates must review upstream release notes, exact versions, MSRV,
advisories, enabled features, licensing and lockfile changes. Model updates also
need provenance, checksums, weight/data licenses and evaluation reports. Do not
automatically fetch a model at application startup.

Original shared code is Apache-2.0; the Slint-linked GUI is GPL-3.0-only. Keep
licensing compatible with the component being changed and retain upstream notices.
The RPM spec is a local development package, not a completed Fedora package review.

Changes to PAM, authselect, service privileges, Polkit or key storage require the
threat model to be updated first. Validate authentication integration in disposable
Fedora VMs; do not use an actual host login stack as a test fixture. Describe
password recovery and rollback in the same change. Never run tests that modify
host authentication without the operator's explicit approval.

Pull requests should state the concrete behavior change, relevant validation and
remaining limitations. Security issues follow [SECURITY.md](SECURITY.md).
