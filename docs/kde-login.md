# Experimental KDE login integration (Fedora)

The `hello-pam/kde-pam` feature extends the existing protected sudo verifier to
root Plasma Login Manager (`plasmalogin`) and SDDM (`sddm`) PAM services. It reuses
the root-owned model files, existing encrypted UID-bound eight-sample templates,
IR camera binding, alignment and three-frame passive matching. No head turns are
required. It does not use GUI/session-D-Bus recognition results as authentication
proof. The sudo adapter continues to work with its previous caller checks.

This Fedora44 host uses Plasma Login Manager6.7.5, with vendor PAM defaults in
`/usr/lib/pam.d/plasmalogin`. `/etc/pam.d/plasmalogin` is an administrator override;
its absence does not mean the display manager lacks PAM configuration. The
manager detects the active systemd display manager and refuses unrelated service
names. Screen locking (`kde`) has a separate explicit opt-in described below.
Fingerprint locking (`kde-fingerprint`), greeter accounts, autologin,
login/sshd/su and authselect's system-auth/password-auth are not enabled by this
integration. The existing running desktop is never stopped/restarted by the
installer or activation tool.

## Build and install

Existing sudo provisioning is required (see [sudo integration](sudo-integration.md)).
A compatible complete V1/V2 reference does not need new enrollment. The normal
user cannot modify the root-owned reference, threshold, keys, models or opt-in.
The graphical helper is root before PAM; it supplies the selected PAM_USER, not a
logged-in real user or sudo PAM_RUSER. UID0/system accounts are rejected. Each
normal account/service needs its own root0600 `pam-SERVICE.enabled` marker with
the exact version1 contents, inside the protected UID directory. Missing,
malformed, linked or writable markers cannot enable face authentication.

```sh
cargo build --release --locked -p hello-pam --features kde-pam \
  -p hello-sudo --features sudo-face
mkdir -p work/kde
cc -Wall -Wextra -Werror -O2 -D_FORTIFY_SOURCE=3 -fstack-protector-strong \
  -Wl,-z,relro,-z,now -o work/kde/kde-pam-probe \
  packaging/kde/pam-probe.c -Wl,-l:libpam.so.0
checkmodule -M -m -o work/kde/linuxhello_kde.mod packaging/kde/linuxhello_kde.te
semodule_package -o work/kde/linuxhello_kde.pp -m work/kde/linuxhello_kde.mod
sudo bash packaging/kde/install.sh "$PWD"
```

The updated helper must also be installed (the KDE installer installs it). The
probe is a root-only diagnostic executable, without setuid/capabilities; its
successful exit grants no operating-system session/command. Its default PAM
conversation supplies no passwords. The explicit development `--password` mode
answers the original PAM password prompt with masked terminal input, never from
arguments/environment/files. It is not used by the verifier or activation.

Fedora's existing `xdm_t` rules already permit reading/writing/ioctls on
`v4l_device_t`. The shipped small SELinux module adds only `chr_file map` for that
same source/target so bounded MMAP capture works. SELinux stays enforcing; no
permissive/unconfined domain, blanket audit2allow or home/key/network access is
added. Only the standalone root-only probe receives a persistent `xdm_exec_t`
entrypoint label so tests run in the same domain as the actual display-manager
helper. Production verifier/worker retain their existing contexts and native
worker sandbox. This policy was checked against this Fedora version; other
policies require their own validation, not automatic permission expansion.

## Activation and use

Close any GUI camera preview and face the original IR camera:

```sh
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py enable --user jan
```

Alternatively run `/usr/local/libexec/linux-hello-sudo/activate-kde.sh` as the
normal desktop user; it supplies your account name and prints the login steps.

The tool first proves a real biometric match with a deny fallback (no password or
other credential can make this probe pass), then checks the exact proposed KDE
PAM stack, account restrictions and credential establishment. Both tests run as
`system_u:system_r:xdm_t:s0-s0:c0.c1023`; they open no desktop session. Tests copy
trusted vendor/administrator PAM files into a private isolated confdir, including
all include/substack dependencies. The original source is backed up under the
root state directory. A preimage check precedes atomic installation of the
administrator override. Failed pre-activation verification leaves login PAM
unchanged and removes a newly created opt-in marker.

Activation labels each of the two scans and counts down three seconds before
starting it. sudo authorization is a separate preceding step. Do not press Enter
repeatedly while capture is running. Expected rejection/timeouts now produce a
short error instead of a Python traceback. The private activation PAM stack uses
the explicit `diagnostic` module option: the same root verifier reports progress,
frame-quality hints, the score, active threshold and rejection reason from that
exact invocation. These messages are displayed when the scan completes. No raw
images or embeddings are logged. The option is restricted to root KDE login
services; production PAM has no diagnostic option and retains suppressed helper
output. Diagnostics never change matching, timeouts, credential checks or the
requirement for three suitable matching frames. A rejected score is not evidence
that lowering the provisional threshold would be safe.

The tool replaces only the authentication `password-auth` substack with:

```pam
# Linux Hello KDE login experimental integration
auth        substack      linuxhello-kde-auth
# Linux Hello original password line: auth        substack      password-auth
```

The original line is retained in a validated comment for exact rollback, including
its spacing. A separate root-owned `/etc/pam.d/linuxhello-kde-auth` contains:

```pam
auth sufficient /usr/lib64/security/pam_linuxhello.so
auth substack password-auth
```

Success is recorded by `sufficient` and ends only the inner substack. Rejection
executes the original password substack. A numeric success jump in the earlier
integration did not record a positive authentication result, so a matched face
could still produce PAM_PERM_DENIED6 when later optional wallet modules could
not authenticate. See the upstream [Linux-PAM control semantics](https://github.com/linux-pam/linux-pam/blob/master/doc/man/pam.conf-syntax.xml)
and [authentication dispatch](https://github.com/linux-pam/linux-pam/blob/master/libpam/pam_dispatch.c).
The manager migrates only the exact previous managed block;
unrecognized administrator modifications are refused. The companion dependency
is validated, installed and relabeled before the service override after both live
tests pass. A companion file with unexpected contents is never overwritten.
Earlier SELinux controls and
later optional wallet/postlogin hooks remain in place; account/password/session
lines are preserved exactly. No failure/partial/unknown-face/error bypasses the
original password stack. Helper cancellation/timeout/cleanup semantics remain the
same as sudo. Closing the PAM parent sends SIGTERM to the helper, whose registered
handler cancels capture and awaits worker/camera/emitter cleanup; a registration
race also rejects. A hard kill/power loss still cannot guarantee emitter restoration.

At the next normal login choose the enrolled account, leave the password field
empty, press Enter or the login button, then face the camera. The native greeter
only starts authentication after that action; this integration does not replace
the KDE theme or silently auto-start scanning. If recognition fails, enter the
normal account password and submit again. Face authentication provides no plaintext
password to unlock an encrypted KWallet; KWallet can request its own password.
Do not store an account password merely to suppress that prompt.

Do not log out/restart the display manager automatically to test this from a
running development session. Keep an authorized root recovery terminal available
when testing a new PAM integration. A full real greeter/session login still needs
the owner to log out deliberately and test the UI.

## Status, rollback and checks

```sh
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py status --user jan
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py disable --user jan
```

Disable restores the saved password line and removes the target user's marker. The
shared companion may remain: without a service reference and account opt-in it
does not enable face login. If the
original config came from the vendor and the remaining content still matches
that vendor file, it removes the override to resume vendor updates. Otherwise it
preserves the administrator's remaining configuration. Backups/metadata remain
root-only. Copying vendor defaults creates an override: review it on package
upgrades. sudo enrollment and activation are independent and remain available.
To remove the optional camera map policy after all graphical integrations are
disabled, an administrator can run `sudo semodule -r linuxhello_kde`.

Development tests before opt-in:

```sh
sudo python3 packaging/kde/test-login.py jan
sudo python3 packaging/kde/test-password-fallback.py jan
```

Negative tests in the real `xdm_t` domain cover missing/corrupt per-service opt-in,
root/system accounts, greeter/autologin exclusion and permit/deny fallback dispatch.
Permit fixtures exist only in isolated tests and grant no desktop/OS command.
The password fallback test temporarily moves the installed detector, invokes the
original KDE password/account/credential stack in `xdm_t`, and restores the model
and marker in `finally`. SIGINT/SIGTERM trigger restoration too; power loss or
SIGKILL requires explicit recovery. It does not change production PAM. Rust tests
also check service/caller scopes and existing timeout/model/template/unknown-face
regressions; Python tests verify exact insertion and reversible removal while
preserving SELinux, wallet, account and session lines.

**Security status:** this is experimental biometric authentication with no
validated presentation attack detection or measured representative NIR FAR/FRR.
The minimum0.80 threshold remains provisional. It must not be described as
Windows Hello-equivalent protection. Model/frame acquisition diagnostics do not
constitute validated anti-spoof evidence.

The PAM adapter's credential callback is a no-op success only after its own
verified match on the same PAM transaction. Every new authentication attempt
clears that marker; failures cannot reuse an earlier match. Returning ignore
unconditionally here caused PAM_CRED_ERR17 in the adapter-only activation probe.
For a development regression of this dispatch (not of recognition), root can run
`python3 packaging/kde/test-setcred.py jan`. Synthetic verifier exits and dummy
state are bind-mounted only in a verified, recursively private mount namespace;
production references/helpers/PAM and desktop sessions remain untouched.

## Unlocking an existing session (Win + L)

KScreenLocker6.7 runs its PAM worker as the logged-in user, using the `kde`
service. It does not run the root display-manager verifier. See the upstream
[PAM worker](https://github.com/KDE/kscreenlocker/blob/Plasma/6.7/greeter/pamauthenticator.cpp).
The optional `hello-lock-verify` launcher is installed root-owned mode4755 on
the ordinary system filesystem. It accepts no arguments or target identity:
the kernel real UID must be a normal account, and its protected root-private
`pam-kde.enabled` must contain the exact opt-in magic. It validates the fixed
helper and profile paths, disables dumps, closes inherited descriptors above2,
clears the environment and execs only `hello-sudo verify REAL_UID` from `/`.
There is no enrollment, diagnostic, command or template export interface.
The existing verifier retains camera/worker isolation and template binding.
Real UID remains the caller's so PAM can cancel its own child. The launcher
grants no privileged shell, OS command or passwordless sudo authority.

After installing the updated module, launcher, probe and management scripts:

```sh
/usr/local/libexec/linux-hello-sudo/activate-lock.sh
```

This performs two real tests as the ordinary selected user, not root/xdm_t,
before backing up and modifying only `/etc/pam.d/kde`. The test configuration
under `/run` is root-owned and readable by that user; it contains PAM lines,
not biometric keys/templates. Root biometric state stays private. The same
substack arrangement preserves password fallback and all other original KDE
PAM groups. Locker diagnostics do not expose root verifier output; only PAM
stage status is reported. Display-manager opt-in remains independent.

Press Win + L, show the unlock screen and face the camera. If authentication
does not start automatically, submit the empty password field. A failed match
continues to the original password prompt. No desktop is locked/restarted by
the installer or activation tool. Status and rollback:

```sh
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py status --service kde --user jan
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py disable --service kde --user jan
```

`test-setcred.py` also tests actual unprivileged locker PAM dispatch and the
installed setuid launcher in its private mount namespace with simulated
verifier exits: own-account success, missing/corrupt opt-in, and rejection.
For a real password fallback check use
`sudo python3 packaging/kde/test-password-fallback.py jan --service kde`.
The presentation attack/NIR qualification limitations above also apply here.
