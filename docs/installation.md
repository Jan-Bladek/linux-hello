# Build, diagnostic installation and removal

Version 0.1.0-dev is a development preview. These steps build real camera
diagnostics and a desktop preview; they do not enable facial authentication. The
PAM, model/PAD, trusted enrollment and TPM release gates remain closed.

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
camera only when preview is requested. A successful compile is not proof that the
target compositor, camera, accessibility stack or Fedora policy works.

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

This version installs no authentication integration, so an authselect rollback is
unnecessary. If future integration is developed, follow the separate
[VM backup/recovery procedure](../packaging/authselect/README.md) before any host
deployment. Never compensate for a bad installation by weakening password policy.
