# Install Linux Hello on Fedora

The recommended full installation builds from source. It prepares the GUI,
YuNet/SFace recognition, sudo/KDE helpers, PAM module and SELinux rules. The older
RPM spec packages diagnostics only. Binary RPM distribution, signed repositories
and package removal/upgrade qualification remain separate work.

## Requirements

- Mutable Fedora 44 or later; KDE Plasma is the tested desktop. Tests ran on Fedora 44.
- A regular desktop account with sudo authorization and a working Polkit agent.
- Internet for the first installation: Fedora packages, Rust, Cargo dependencies and models.
- Free space and time to compile the GUI/OpenCV integration with Rust 1.99.0.
- A compatible camera. Automatic emitter control is limited to the
  [tested hardware](hardware-support.md), not every camera advertised as infrared.

The installer does not support Fedora Atomic/ostree or Debian/Ubuntu. Sign-in and
screen unlock support KDE and [Hyprland / Noctalia / Hyprlock](hyprland-noctalia.md); select only the uses you need.
Representative NIR FAR/FRR and reviewed PAD qualification remain open. Keep your
password available.

## Install

If you do not have the repository:

```sh
sudo dnf install git python3
git clone https://github.com/Jan-Bladek/linux-hello.git
cd linux-hello
./install.sh
```

**Run the installer without sudo.** It requests authorization for Fedora packages
and system files. Rust, Cargo, model downloads, tests and compilation run as your
regular user. Do not compile the project as root.

The installer:

1. Checks Fedora and installs native development/runtime dependencies with DNF.
2. Uses existing rustup or downloads the official Rust installer to a private user
   directory. It selects 1.99.0 without changing the global default toolchain.
   See [official Rust installation](https://doc.rust-lang.org/stable/book/ch01-01-installation.html).
3. Runs GUI/helper/PAM tests and documentation checks, then builds all required
   binaries from `Cargo.lock` with recognition and KDE feature gates.
4. Validates the pinned models' byte sizes and SHA256 digests. It reuses a valid
   local cache or explicitly downloads artifacts and upstream notices. Models
   are not downloaded by the application during authentication.
5. Builds the hardened PAM probe and minimal camera MMAP policy. Checks the
   private stage before installation, atomically replaces files and restores
   earlier files on ordinary failure. Retains installed policies and never
   switches SELinux to Permissive.
6. Adds **Linux Hello** to the desktop menu and installs `linux-hello` and `hello`.

Installation never enrolls a face, changes `/etc/pam.d`, activates an account or
starts the diagnostic daemon. An update retains existing profiles, keys, thresholds
and opt-ins. Other accounts' references are not deleted.

For an existing Hyprland + Noctalia desktop, use
[`./install-hyprland-noctalia.sh`](install-hyprland-noctalia.md) for desktop
preflight and optional ordered activation. It delegates to this full installer.

## First run and activation

Open **Linux Hello** from KDE's menu or run:

```sh
linux-hello
```

Scan your face, name it, select **Sudo**, **Desktop sign-in** and/or **Screen unlock**,
then **Save and set up**. Choose a separate reference passphrase of at least 12
characters and confirm it. For an existing encrypted reference, use its original
passphrase. The separate system authorization dialog uses your Linux account
password, not the reference passphrase.

Setup runs live face/PAM checks before activation. Close camera preview, face the
camera and wait for the result. Ordinary failure restores the previous system
state. See [unified setup](kde-login.md#unified-gui-setup).

Updates do not require re-enrollment. Close the old window and reopen the installed
application. The installer does not log out, lock the session or restart the
display manager. Account controls in Overview/Settings can inspect, pause or
remove your protected system reference; see [account management](account-management.md).

## Options

| Command | Behavior |
| --- | --- |
| `./install.sh --dry-run` | Show a plan without network access, compilation or changes |
| `./install.sh --build-only` | Prepare dependencies, tests, binaries and staging without installing application files |
| `./install.sh --skip-deps` | Skip DNF; native dependencies must already be installed |
| `./install.sh --skip-deps --offline` | No network; requires toolchain, Cargo cache, native libraries and valid models |
| `./install.sh --with-tpm` | Also install the optional TPM access policy for KDE sign-in |

`--build-only` alone can install dependencies with DNF. Add `--skip-deps` to skip
that step. Example for an already provisioned workstation:

```sh
./install.sh --build-only --skip-deps --offline --with-tpm
```

`--with-tpm` neither migrates keys nor resets the TPM. After reference provisioning
and successful activation, an administrator can explicitly migrate the existing key:

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-enable "$(id -u)"
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-status "$(id -u)"
```

Migration does not require another scan. Read [TPM limits and recovery](tpm.md)
first. Updates without `--with-tpm` retain an already sealed profile and installed
TPM policy. This mode does not attest boot state.

## Files, upgrades and recovery

Binaries, workers, models and helpers live in `/usr/local/libexec/linux-hello-sudo/`;
PAM lives in `/usr/lib64/security/pam_linuxhello.so`. Commands are in
`/usr/local/bin/`, the system shortcut in `/usr/local/share/applications/`, and the
current user's shortcut in `~/.local/share/applications/`. An earlier user shortcut
is backed up as `linux-hello.desktop.before-install`.

Root-private `install-manifest.json` records source revision and file hashes.
A `-dirty` suffix means the build included local modifications. Biometric profiles
remain in `/var/lib/linux-hello/sudo/`; encrypted GUI references stay in the user's
data directory. Private staging is removed on completion. Source, crate and model
caches remain available for later builds.

Update:

```sh
git pull --ff-only
./install.sh
```

Ordinary installation errors restore earlier files and remove only newly added
context rules/policies. This is not an atomic multi-file transaction across power
loss or SIGKILL. Empty directories may remain.

Before removing helper/module files, disable integrations for every account that
uses them using [sudo rollback](sudo-integration.md) and [KDE rollback](kde-login.md).
The installer never automatically erases references, keys or administrator settings.
It is not RPM-managed: `dnf remove linux-hello` does not remove these source-installed
files. Pausing one account is separate from globally removing the application.

## Executed validation

Full build/staging and upgrade passed on the existing Fedora 44 host with cached
dependencies/models. Installer tests cover unsupported distributions, changed
files, model pins, links/path injection, rollback and inert dry-run. An isolated
file-install test used empty installation directories and existing SELinux policies,
without real profiles or PAM activation. Fresh-VM dependency/bootstrap downloads,
new policy installation and RPM lifecycle remain unvalidated. See [validation](validation.md).

After an audit-remediation upgrade, explicitly reactivate existing PAM
integrations to migrate their known managed blocks. Unknown MFA/access/lockout
policies are refused. The old partial sudo/KDE install scripts are retired.
See [audit remediation](audit-remediation.md#recovery-and-upgrade).

For Noctalia 5.x or Hyprlock session unlocking, see
[Hyprland and Noctalia activation](hyprland-noctalia.md).
