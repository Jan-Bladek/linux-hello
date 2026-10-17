# Complete Linux Hello installer for Hyprland + Noctalia

Use `install-hyprland-noctalia.sh` to install the **whole Linux Hello application**
on an existing mutable Fedora 44+ Hyprland / Noctalia 5.x desktop. This includes
the GUI, recognition worker, pinned YuNet/SFace models, sudo/desktop PAM adapter,
setuid locker launcher, management tools and SELinux integration. It delegates
building and system-file installation to the same reviewed
[full Fedora installer](install-fedora.md), avoiding a second deployment path.

This is an application installer, not a Fedora image or compositor installer.
Install Hyprland and Noctalia through your desktop's package workflow first.
The script checks `/usr/bin/Hyprland` and `/usr/bin/noctalia`, including the
Noctalia version, before downloading dependencies or changing system files.
It does not enable third-party repositories, replace your display manager,
install a dotfiles distribution or rewrite your Hyprland keybindings.
Fedora Atomic/ostree, Debian/Ubuntu, Noctalia 4 and greetd/Noctalia Greeter PAM
activation are outside this installer's support.

## First installation

Run these commands as your normal desktop user:

```sh
sudo dnf install git python3
git clone https://github.com/Jan-Bladek/linux-hello.git
cd linux-hello
./install-hyprland-noctalia.sh --open-settings
```

**Do not prefix the installer with sudo.** It requests authorization for DNF and
fixed-path system installation. Rust, tests, compilation, model download and
staging remain unprivileged. Default installation does not activate PAM.
The optional `--open-settings` starts the installed GUI attached to the terminal
after installation; close its window when finished. Omit it on a non-graphical
terminal and open `linux-hello` later inside your Noctalia session.

The workflow is:

1. Check the Fedora target, installed Hyprland and Noctalia 5.x.
2. Install native development/runtime dependencies through DNF. Bootstrap the
   pinned Rust toolchain when needed, without changing the global default.
3. Test the GUI/helper/PAM code and documentation, build with locked Cargo
   dependencies and all required recognition/desktop features.
4. Validate exact model sizes and hashes, reuse valid caches or download the
   pinned artifacts and licenses. Build the PAM probe and minimal SELinux policy.
5. Validate private staging, request sudo authorization and atomically install the
   complete bundle. Preserve references, keys, thresholds, existing opt-ins and
   password fallback. SELinux remains enforcing.
6. Check the installed unprivileged desktop adapter. Open the GUI if requested.

On first run, scan your face, give it a name, select **Sudo**, **Desktop sign-in**
(if using PlasmaLogin/SDDM) and **Screen unlock**, then **Save and set up**.
Use a separate reference passphrase; the account password belongs only in the
system authorization dialog. A working session Polkit agent is required for that
GUI authorization. All selected integrations perform real face checks.
The GUI detects a running Noctalia process owned by you; run this step inside your
Noctalia session. Do not claim enrollment success from merely installing software.

After successful Noctalia activation, deliberately lock using your existing
shortcut or `noctalia msg session lock`. Submit the empty password field once
and face the camera. A mismatch falls back to your normal password. Initial
Hyprland session sign-in still uses your existing display manager.
The installer never locks/logs out/restarts your session to test this.

## Existing protected reference: install and activate

If your protected system face reference already exists, this explicitly installs
or updates the application and activates Noctalia unlock after its two live checks:

```sh
./install-hyprland-noctalia.sh --activate-unlock
```

To also activate the currently selected PlasmaLogin/SDDM sign-in service:

```sh
./install-hyprland-noctalia.sh --activate-login --activate-unlock
```

For this already provisioned workstation, use cached build inputs without DNF:

```sh
./install-hyprland-noctalia.sh --skip-deps --offline --with-tpm --activate-unlock
```

If you have saved an encrypted **local GUI reference** but have not imported it
for system authentication, explicitly request the existing enrollment helper:

```sh
./install-hyprland-noctalia.sh --enroll-sudo --activate-login --activate-unlock
```

`--enroll-sudo` imports the saved reference, asks for its passphrase through the
existing masked prompt, verifies your real face and enables sudo. It does not
create a reference from nothing and is not necessary for an already protected
reference. Do not use it just to update software. No name/UID/template/path or
password is accepted as an installer argument.

Setup order is enrollment/sudo → display-manager login → Noctalia unlock.
Each action is opt-in and uses the existing installed managers, preserving their
subject checks, two-stage locker tests and password fallback. Noctalia's
empty-submit overlay is installed as your ordinary user only after successful
unlock activation. Later GUI overrides are detected, not silently rewritten.

The whole sequence is **not one transaction**. If a later action fails, the
installation and earlier successful activations remain; no later action or GUI
launch runs. Each existing manager handles its own ordinary rollback. A rejected
face never counts as successful activation. See
[desktop scope and rollback](hyprland-noctalia.md#scope-and-rollback).

## Options and update

| Option | Effect |
| --- | --- |
| `--dry-run` | Print the selected plan; no subprocesses, downloads or writes |
| `--build-only` | Prepare tests/build/models/staging without installing application files; cannot combine with activation or GUI launch |
| `--skip-deps` | Skip DNF; dependencies must already exist |
| `--offline` | Use cached Rust/crates/models; requires `--skip-deps` |
| `--with-tpm` | Include optional TPM policy; no key migration, TPM reset or boot-state attestation |
| `--enroll-sudo` | Explicit protected import of an existing encrypted GUI reference and live sudo activation |
| `--activate-login` | Two live checks and activation of the existing supported display manager |
| `--activate-unlock` | Two live checks and Noctalia unlock setup |
| `--open-settings` | Launch the installed GUI after the requested work succeeds |

Preview or validate a cached build:

```sh
./install-hyprland-noctalia.sh --dry-run --activate-unlock
./install-hyprland-noctalia.sh --build-only --skip-deps --offline --with-tpm
```

`--build-only` alone can still install native dependencies through DNF; add
`--skip-deps` to prevent that. Updates retain existing profiles and opt-ins:

```sh
git pull --ff-only
./install-hyprland-noctalia.sh
```

Updates do not require re-enrollment. Existing TPM-wrapped keys and installed
policies remain available even when `--with-tpm` is omitted. See
[TPM migration and limits](tpm.md) for deliberate key migration, and
[installation recovery](install-fedora.md#files-upgrades-and-recovery) before
removing deployed PAM helpers. No validated PAD or representative NIR FAR/FRR
qualification is implied by this installer.

## Executed checks

Profile tests cover inert dry-run, rejected root/setuid callers, missing/unsupported
desktops, forwarding of build options, opt-in-only activation, setup ordering,
stop-on-failure and build-only exclusion of installed helper execution. The
full build/staging profile was exercised offline on the existing Fedora 44
Hyprland / Noctalia 5.3.0 workstation. Fresh-machine package/bootstrap downloads
and a newly created desktop installation were not exercised. Existing PAM tests
cover the adapter separately; an installer test does not prove physical unlock.
