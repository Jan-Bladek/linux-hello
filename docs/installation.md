# Installation, builds and removal

For the complete current Fedora application, use `./install.sh` as the ordinary
desktop user. It builds face-enabled GUI/CLI and KDE-enabled PAM, verifies model
hashes and installs the helpers without activating any account or editing PAM.
The [full installer guide](install-fedora.md) includes dependencies,
first run, options, TPM, upgrades and test limits. The remaining sections describe
manual diagnostic builds and the separate diagnostic-only RPM specification.

Version 0.1.0-dev is a development preview. These steps build real camera
diagnostics and a desktop preview; they do not enable facial authentication. The
production model/PAD and measured-boot qualification gates remain closed. [TPM key wrapping](tpm.md) is required for new system profiles; existing legacy
providers need explicit migration. A separate explicit
experimental sudo/KDE login/unlock opt-ins are linked at the end of this document.

## Fedora development build

Use a regular account and a supported Fedora environment. Install build libraries
through the administrator's normal package process:

```sh
sudo dnf install gcc clang-devel pkgconf-pkg-config systemd-devel dbus-devel \
  dbus-daemon fontconfig-devel freetype-devel libxkbcommon-devel wayland-devel \
  libX11-devel libXcursor-devel libXi-devel libXrandr-devel mesa-libGL-devel
```

Install the Rust 1.99.0 toolchain through an approved Rust distribution. If rustup
is already installed, `rustup toolchain install 1.99.0 --profile minimal --component
rustfmt --component clippy` installs the pinned tools. The repository's
`rust-toolchain.toml` selects them. Record `rustc --version` and `cargo --version`.

From the source root:

```sh
cargo build --locked -p hello-cli -p hello-daemon
cargo build --locked -p hello-settings
cargo test --workspace --locked
bash tests/integration/run-session.sh
target/debug/hello --help
target/debug/hello security
target/debug/hello cameras
target/debug/hello-settings
```

The default workspace build omits the GUI; use `--workspace` or its explicit package
name to include it. The Slint GUI runs under the logged-in user and opens the
camera only during an explicit preview, diagnostic, enrollment or recognition operation. A successful compile is not proof that the
target compositor, camera, accessibility stack or Fedora policy works.

For a recorded local preflight, run `python3 tools/qualify-linux.py` from the
source tree. It records Fedora/kernel/desktop, machine type and BIOS version,
camera-node visibility and GUI development-library availability, without opening
a camera. Reports and command logs go to a new private directory under ignored
`work/`; serial numbers and camera pixels are not collected. Exit status 1 means
the preflight found blockers; inspect `report.json`.

Run `python3 tools/qualify-linux.py --verify` to also build the full workspace,
check formatting and Clippy, run unit and isolated-bus tests, and collect CLI
security/camera diagnostics. If the build fails, runtime checks are marked
blocked and the recorder still runs tests and Clippy for core, face, IPC,
fingerprint, PAM and daemon. This partial result does not pass full qualification;
camera, storage, CLI and GUI remain outside that fallback. Use `--offline` when
the complete locked dependency graph is cached.
An offline failure naming a missing crate requires fetching that exact locked
dependency from a terminal with network access; do not change `Cargo.lock`.

If bindgen reports that it cannot find `libclang`, install the documented
`clang-devel` build prerequisite. GUI errors naming `fontconfig`, `freetype2`,
`xkbcommon` or `wayland-client` require their development packages listed above.
These checks install no system packages.

## First hardware checks

Use a video node reported by `hello cameras`; `/dev/video0` below is an example,
not a prediction of the Lenovo's camera layout:

```sh
target/debug/hello capture --device /dev/video0 --frames 10 --timeout 10
target/debug/hello probe --device /dev/video0
target/debug/hello fingerprints
```

Capture reports metadata and discards pixels. The probe chooses the initial Lenovo
test profile at the operator's request; it does not auto-detect the laptop model
or certify IR/liveness. Read [hardware support](hardware-support.md) before calling
any device compatible. If permissions are denied, inspect the current session's
device ACLs and policy. Do not change device modes, grant blanket video-group
membership or run the GUI as root to hide the error.

fprintd remains the owner of fingerprint readers, templates and authorization. The
diagnostic adapter does not enroll/delete prints or edit existing fprintd PAM
configuration. A missing service or unsupported reader is a diagnostic result.

## Unprivileged diagnostic daemon

A development session bus is sufficient to inspect the read-only protocol:

```sh
dbus-run-session -- bash
```

Inside that temporary shell, start `target/debug/hello-daemon --session &`, note its
PID, and run `target/debug/hello status --session`. Stop that process with `kill`
using its recorded PID and leave the shell. The entire session is disposable; no
system service or authentication settings change. A session-bus instance has no
authority to authenticate a user.

The interface is `org.linuxhello.Daemon1` at `/org/linuxhello/Daemon1`. It exposes
`GetVersion`, `GetCapabilities`, `GetSecurityStatus` and `GetRequesterUid`. The last
method resolves the actual sender through the bus rather than accepting a UID.
It returns only the requesting connection's UID.

## Building local RPMs

The spec is a development build configuration, not a completed Fedora package
review. Use a disposable Fedora build VM with `rpm-build`, the spec's
BuildRequires and packaged Rust/Cargo >= 1.99. No RPM build or installation on
Fedora is claimed until its validation report exists. The spec intentionally
builds offline from vendored dependencies.

Prepare the source outside the repository, using a new empty directory:

```sh
SOURCE_OUTPUT=$(mktemp -d)
RPM_TOPDIR=$(mktemp -d)
bash packaging/fedora/prepare-source.sh "$SOURCE_OUTPUT"
mkdir -p "$RPM_TOPDIR/SOURCES"
cp "$SOURCE_OUTPUT/linux-hello-0.1.0-dev.tar.gz" "$RPM_TOPDIR/SOURCES/"
rpmbuild -ba --define "_topdir $RPM_TOPDIR" packaging/fedora/linux-hello.spec
```

Choose task-specific paths not shared with other users for a real build. The
source helper refuses an output directory inside the repository and refuses to
overwrite an existing archive. It vendors the locked Rust graph and downloads no
models. `rpmbuild --without gui ...` omits the optional GUI subpackage. The daemon
subpackage remains independently installable.

Review the resulting package contents with `rpm -qpl`, scriptlets with `rpm -qp
--scripts`, dependencies with `rpm -qpR`, and licenses/source notices before
distribution. Publish matching complete source, vendored dependency notices and
required corresponding source with any GUI binary. See [research](research.md).

## Optional system diagnostics in a Fedora test VM

Installing the CLI RPM does not install the daemon. The optional daemon RPM creates
an unprivileged `linux-hello` account and installs a hardened unit and system-bus
name policy. It does not start or enable the service, install D-Bus activation,
Polkit actions or PAM, select authselect, or change encryption.

After deliberately installing the locally built diagnostic daemon RPM in a test VM,
an administrator can run:

```sh
sudo systemctl start linux-hello.service
hello status
systemctl status linux-hello.service
sudo systemctl stop linux-hello.service
```

There is no `[Install]` unit section and no boot enablement workflow in this
preview. The service uses `PrivateDevices=yes` and cannot capture a camera. Camera
diagnostics stay in the ordinary user's CLI/GUI. The system-bus service's name is
restricted to its dedicated account and root; only public diagnostic methods are
permitted for other callers.

Test with SELinux enforcing. Record relevant AVC denials and correct labeling or
policy in a reviewable change. Do not disable SELinux or generate broad allow rules
as an installation step. On service failure, password authentication is unaffected.

## Validation matrix before distributing an RPM

| Test | Expected result |
|---|---|
| Install CLI only | `hello security` and device enumeration work without root |
| Install GUI | Launch on intended Wayland desktop; no capture until requested |
| Install daemon | Unit remains inactive; no autoactivation on `hello status` |
| Start daemon explicitly | Runs as `linux-hello`, empty capabilities, diagnostics work |
| Other user attempts name ownership | System bus denies ownership |
| Camera under hardened daemon | Inaccessible; no camera API exists |
| Inspect package file list | No `/etc/pam.d`, active Polkit policy, authselect profile or activation file |
| Upgrade inactive service | Remains inactive; no authentication changes |
| Upgrade manually running service | No surprise authentication path; operator restarts deliberately |
| Remove packages | Service stops, package files removed, existing PAM/fprintd unchanged |

Save RPM names, Fedora/kernel/desktop versions, enforcing-mode result, command
exit codes and sanitized observations. Do not save raw frames or sensitive data.

## Removal and rollback

For the RPM installation, stop a deliberately started service first, then remove
only the installed Linux Hello packages using `dnf remove linux-hello
linux-hello-gui linux-hello-daemon`. Inspect the DNF transaction before accepting it.
The daemon RPM also stops the service on final removal and reloads systemd. The
dedicated non-login account is intentionally retained to avoid UID-reuse problems.
No existing fingerprint records, user home data or PAM configuration are deleted.

For a source build, exit the GUI and stop only the daemon PID you started. Binaries
are under `target/`; no system uninstall is needed because the build did not install
anything. Keep any explicitly created private library test data separate from
the repository and remove it through the owning test's cleanup or reviewed path.

The default diagnostic installation requires no PAM rollback. Manual optional
authentication is separate: disable its service-specific integrations before
removing their helper/module files, using [sudo rollback](sudo-integration.md) and
[KDE rollback](kde-login.md). Removing diagnostic RPMs does not disable a manually
installed PAM adapter. Authselect profiles remain untouched. See the separate
[VM recovery procedure](../packaging/authselect/README.md) for future profile changes.

## Optional face diagnostics

The default build and packages do not require OpenCV or weights. For the explicit
`face-opencv` build, model provenance and live `face-probe` usage, see
[inference diagnostics](inference.md). This provides inference tools without enabling PAM; GUI face enrollment requires
its explicit `face-diagnostics` feature and user actions.

The optional build produces both `hello` and `hello-face-worker`; keep these
executables in the same directory. Default RPM packages do not enable this feature
or distribute weights. Do not install only the optional CLI without its worker.

### Explicit experimental authentication

The default diagnostic installation above does not authenticate users. A separate
optional root-owned verifier and PAM adapter can be installed and enabled after
protected reference provisioning and live verification. Start with sudo, then
optionally add KDE display-manager login and session unlocking independently. See
[sudo activation, limitations and rollback](sudo-integration.md). This opt-in
preserves password fallback and does not change authselect/system-auth. It has
no validated presentation attack detection or measured representative NIR FAR/FRR.
KDE setup, including the self-only locker launcher, is documented in
[KDE login and Win + L](kde-login.md). Enabling sudo does not enable KDE automatically.

## Full GUI setup

After sudo/KDE helper installation, the full GUI offers **Save and set up**
with selectable sudo, KDE login and Win + L cards. `pkexec` and a working Polkit
agent are required. The KDE installer installs `configure-person.py`; no separate
manual activation is needed for the selected uses after the GUI live checks pass.
See [unified setup](kde-login.md#unified-gui-setup). The developer switch exposes
local-only saving and advanced tools. Existing script-based activation/rollback
remains available. Replacing a face preserves a stricter existing root threshold.

For Noctalia 5.3.0 or Hyprlock session unlocking, see
[Hyprland and Noctalia activation](hyprland-noctalia.md).
