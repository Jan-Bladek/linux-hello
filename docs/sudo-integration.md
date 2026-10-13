# Experimental sudo face authentication

This optional Linux integration reuses the working IR camera, pinned YuNet/SFace
worker, eight-sample V1/V2 templates, alignment and three-frame passive 1:1
verification. It does not require head turns during authentication. It is a real
PAM authentication path when explicitly enabled, separately from the diagnostic
GUI and session D-Bus daemon. Existing PAM account/session/password includes,
sudoers authorization and password fallback remain in place. This sudo installer
does not activate KDE, tty login, sshd, su or change system-auth/authselect.
KDE login/unlock are separate [opt-ins](kde-login.md).

**Security limitation:** presentation attack detection is not validated. Photos
or recordings have not been qualified against this implementation. The existing
0.80 minimum is provisional, not a measured FAR target on this NIR camera. This is
an explicit experimental opt-in, not Windows Hello-level security. Do not infer
FAR/FRR from unit tests. Mismatch or a model/camera error cannot grant biometric
authentication; the false-accept rate for unseen people remains unmeasured.

## Installation and activation

On Fedora with the existing OpenCV build dependencies and downloaded, pinned
models in `work/inference/models`:

```sh
cargo build --release --locked -p hello-sudo --features sudo-face \
  -p hello-pam --features sudo-pam -p hello-cli --features face-opencv
sudo bash packaging/sudo/install.sh "$PWD"
```

If KDE login or unlock is already enabled, build `hello-pam` with `kde-pam`
instead of `sudo-pam` before reinstalling the shared module. The KDE feature
includes sudo support; a sudo-only/default module would remove KDE support.

Installation copies executables, models and the PAM adapter into root-owned
system directories. It does **not** edit PAM. Do not run inference or install the
PAM module directly from a user-writable repository directory.

Use the existing GUI to enroll and save an eight-sample IR reference. The account
must be a normal UID >=1000, and the original camera must be present. Close the
GUI camera preview, then run as your normal desktop user:

```sh
/usr/local/libexec/linux-hello-sudo/enroll-and-enable.sh
```

The script requests fresh sudo authorization, then reads the **GUI reference
passphrase** from `/dev/tty` with echo disabled. This is a different prompt from
the account's sudo password. Never pass this phrase on the command line, through
chat, an environment variable or a file. Import verifies user-owned private files
without symlink traversal and does not alter the original GUI store. It requires
root and decrypts the envelope bound to the intended account UID. A random root
key encrypts the imported template under `/var/lib/linux-hello/sudo/<uid>` (0700;
key, config and template 0600). The account name comes from NSS, not a display
label. A local-only save does not change the sudo copy. The unified GUI **Save and set up** deliberately updates it after system authorization and live checks.
See [unified setup](kde-login.md#unified-gui-setup).

The installed verifier confirms the imported reference with live IR frames before
enabling PAM. `manage.py enable --uid UID` requires a second live match and backs
up the original sudo file before an atomic, preimage-checked change. Only this
block is added before the original `auth include system-auth`:

```pam
# Linux Hello sudo-only experimental integration
auth       sufficient   /usr/lib64/security/pam_linuxhello.so
```

Fedora `sudo-i` already includes `sudo`. This sudo path applies only to these two PAM service names and the caller's
claimed account. The optional kde-pam feature adds independently gated KDE paths. sudo may run PAM with real
UID0: in that case the adapter uses PAM_RUSER supplied by sudo's trusted plugin;
it never trusts SUDO_USER or user environment values. Missing PAM_RUSER when the
real UID is root causes password fallback. Root/system account templates are not
supported. No enrollment or camera availability means immediate password fallback.

An authorized face update preserves an existing stricter root threshold. The
unified GUI can disable sudo for its own account through a root-private
`pam-sudo.disabled` marker; any existing marker or metadata error rejects this
biometric path. Absence preserves legacy enrolled profiles. This does not remove
the global PAM line or another account's enrollment.

## Verification and failures

The adapter reads only PAM_SERVICE/PAM_USER/PAM_RUSER, never AUTHTOK. It launches
one fixed, root-owned helper with a cleared environment. Every ancestor of helper,
models, keys and configuration must be root-owned and not group/other writable;
files must be regular, non-symlink, single-link and size-bounded. The root helper
loads one protected UID-bound template, checks its version, model, completeness,
geometry and original IR camera binding, then executes the existing isolated
native worker. Model digests and input processing remain unchanged. All three
fresh valid frames must pass the configured threshold (minimum0.80). There is no
GUI result/cache accepted as proof and no partial-scan success.

An acquisition/model/template/cleanup error, mismatch, ambiguity, cancellation or
signal cannot return biometric PAM_SUCCESS. A normal matching helper exit is the
only success path. The adapter returns PAM_IGNORE on every other outcome so the
original password stack proceeds. At25s it requests graceful cancellation; after
another10s it kills/reaps a hung helper and still rejects. Normal cleanup awaits
camera stop, emitter readback/restoration and worker exit before acceptance.
Forced SIGKILL/power loss cannot guarantee emitter restoration. The helper disables
core dumps; it stores/logs neither raw live images nor biometric vectors.

Diagnostics require root and never relax matching controls:

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo diagnose 1000
sudo /usr/local/libexec/linux-hello-sudo/manage.py status
```

They report camera format, latency, quality, similarity, threshold, result and
cleanup status. Re-importing replaces the root template only after fresh admin
authorization; compatible existing full eight-sample V1/V2 references need no new
scan. Changing model families requires compatible new enrollment; no embedding
mixing or automatic conversion is introduced.

## Recovery and testing

To remove only the integration block while preserving other PAM edits:

```sh
sudo /usr/local/libexec/linux-hello-sudo/manage.py disable
```

The root-only `sudo-before-<hash>.pam` backup remains in the state directory.
Keep an authorized root terminal available while testing a new PAM stack. After
activation use `sudo -k; sudo id -u` facing the camera, then repeat with the lens
covered and confirm that the original password still works. A successful command
using a cached sudo timestamp does not test recognition. No production fallback
success or anti-spoof qualification is inferred from synthetic tests.

Before enrollment, the root-only `packaging/sudo/test-pam.py USER` uses Linux-PAM's
isolated `pam_start_confdir` with temporary permit/deny fixtures. These fixtures
are never installed in `/etc/pam.d` and authorize no operating-system command.
They test missing enrollment/module rejection, original-stack dispatch and
service restrictions. Rust tests cover account/service restrictions, insecure
paths, corrupt/cross-UID configuration, invalid thresholds, encrypted template
integrity and administrator import. Existing recognition regression tests cover
unknown faces, multiple faces, duplicate/poor frames, cancellation and timeouts.
Physical positive recognition and covered-camera password fallback require the
owner's presence after protected provisioning; representative NIR FAR/FRR and
presentation attacks remain unvalidated.

A separate explicit [`kde-pam` opt-in](kde-login.md) can now reuse the same protected
reference for KDE display-manager login and Win + L unlocking. It has independent per-service/account
markers and PAM activation; enabling sudo alone never enables graphical login.

## Optional hardware key protection

An existing imported profile can explicitly migrate its key to a local TPM without
changing its face samples or thresholds. See [TPM key protection](tpm.md) for
commands, enforcing SELinux checks, recovery and limitations. The normal legacy
key remains supported for profiles that have not migrated.
