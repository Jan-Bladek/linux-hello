# Hyprland, Noctalia and Hyprlock (Fedora)

Hyprland is a compositor. Initial session sign-in still belongs to the display
manager: use the existing [PlasmaLogin / SDDM integration](kde-login.md). Choosing
Hyprland instead of Plasma does not require another enrollment or a different
PAM stack. This adapter does not install or configure greetd / Noctalia Greeter.

For an existing session there are two supported locker paths:

| Locker | PAM service | Original authentication fallback |
| --- | --- | --- |
| Noctalia 5.x (reviewed source: 5.3.0) | `login` | The host's existing `system-auth` substack |
| Hyprlock (default service name) | `hyprlock` | Reviewed Fedora `system-auth` substack |

Noctalia 4 / Quickshell's `system-login` interface is not supported by this
adapter. Noctalia 5.3.0 hardcodes `login` in its
[lock-screen implementation](https://github.com/noctalia-dev/noctalia/blob/3982122906f2409aeaff17399552153c4c323f20/src/shell/lockscreen/lock_screen.cpp).
Its [PAM helper](https://github.com/noctalia-dev/noctalia/blob/3982122906f2409aeaff17399552153c4c323f20/src/auth/pam_authenticator.cpp)
runs as the session owner. Hyprlock defaults to `hyprlock`, with a `su` fallback
when that service is missing; see the [upstream manual](https://wiki.hypr.land/Hypr-Ecosystem/hyprlock/).
Linux Hello creates a dedicated `hyprlock` stack if none exists, leaving `su`
unchanged. An existing nonstandard stack requires review instead of replacement.

## Install and enable

The dedicated [complete installer](install-hyprland-noctalia.md) handles native
dependencies, Rust, tests, models and the same full application bundle:

```sh
./install-hyprland-noctalia.sh --open-settings
```

Run it as your normal account. The generic [full installer](install-fedora.md)
remains available. The existing
`hello-pam/kde-pam` feature name is retained for compatibility; it now includes
these self-account desktop lockers. Default diagnostic builds remain unauthoritative.
No new model or re-enrollment is needed for a valid protected V1/V2 reference.

The full GUI's **Screen unlock** selection detects a running Noctalia process
owned by your UID; otherwise Hyprland selects Hyprlock and other desktops select
KDE. This is a UI suggestion, never proof of identity. The root manager independently
checks the service whitelist, authorization and real face verification. Previously
enabled lockers remain enabled when adding another one. **Pause**, **Remove**,
or deselecting **Screen unlock** revokes every supported locker for your account.

For an existing protected reference, run as your normal desktop account:

```sh
# Detect the running locker:
/usr/local/libexec/linux-hello-sudo/activate-desktop.py
# Or explicitly choose one:
/usr/local/libexec/linux-hello-sudo/activate-desktop.py --service noctalia
/usr/local/libexec/linux-hello-sudo/activate-desktop.py --service hyprlock
```

Close the camera preview. Sudo authorization happens first, followed by two
independent, real face checks with a countdown. One proves the biometric adapter
with a deny fallback; the other checks the proposed original parent/account/
credential stack. Tests run in a private mount namespace as the unprivileged
account, with a private copy of the profile. Production opt-in is installed only
after successful tests. Failure retains the previous PAM configuration.

For Noctalia, the normal-user helper enables only
`[lockscreen] allow_empty_password = true` in an owned
`linux-hello.toml` overlay under the shell's XDG / `NOCTALIA_CONFIG_HOME`
configuration directory. This permits Enter on an empty field to **start PAM**;
a face match or the original password path must still authenticate. It never
enables a grace period or disables locking. Existing unrelated files are retained.
If GUI-managed settings override the overlay, enable **Allow empty password**
in Noctalia's Security settings; the helper reports the conflict and removes its
new ineffective overlay. See the pinned upstream
[configuration layering](https://github.com/noctalia-dev/noctalia/blob/3982122906f2409aeaff17399552153c4c323f20/docs/user/configuration/index.mdx).

Lock deliberately using your existing shortcut or `noctalia msg session lock`.
Submit the empty password field once, face the camera and wait. No head turn is
required. If recognition fails, enter your normal account password. The installer
and activation never lock the session, log you out or restart the display manager.
Noctalia settings may require a normal shell restart if configuration reload is
not active; do that deliberately, outside a locked session.

## Scope and rollback

Noctalia shares a PAM name with console login. Linux Hello accepts `login` only
when the normal claimed UID equals the kernel **real** caller UID. Root/system
TTY login processes are rejected by this biometric module and continue through
the existing authentication policy. Root privilege alone cannot use the locker
path. Another UID, an absent/corrupt opt-in, a model/camera error, timeout or an
unsuitable face cannot grant a match.

The fixed root-owned setuid launcher accepts only `--service kde`,
`--service hyprlock` or `--service login`; the legacy no-argument KDE call remains
supported. It derives identity from the real UID, not an argument or environment.
Separate root-private `pam-SERVICE.enabled` markers prevent one locker opt-in
from enabling another. No privileged shell, command choice or template export
is exposed. Existing setup locks, revocation epochs, runtime policy checks,
TPM wrapping and isolated native workers remain in use.

Only the original `auth substack system-auth` line is wrapped with a service-specific
companion (`linuxhello-login-auth` or `linuxhello-hyprlock-auth`). The saved line,
postlogin, account, password and session rules remain intact. Unknown MFA/lockout
policies fail closed rather than silently bypassing those controls. Shared
`system-auth` / `password-auth`, `su` and authselect are never edited.

```sh
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py status --service login --user "$USER"
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py disable --service login --user "$USER"
sudo /usr/local/libexec/linux-hello-sudo/manage-kde.py disable --service hyprlock --user "$USER"
```

Disable affects only your opt-in; other enrolled accounts retain theirs. After
the last opt-in, restore the saved line. An installer-created Hyprlock file is
removed only if it still exactly matches the reviewed default after removal;
administrator changes are preserved. The normal-user Noctalia overlay can be
removed separately if desired; its empty-submit option grants no authentication
by itself. These operations do not erase the protected face reference.

## Validation limits

Automated tests cover reversible PAM edits, protocol/service restrictions,
Noctalia overlay conflicts and all-locker revocation. The root-only
`packaging/kde/test-setcred.py` additionally exercises actual unprivileged PAM
and the setuid launcher in a verified private namespace: successful simulated
helper exits, mismatches, missing/corrupt per-service markers and root TTY rejection.
These fixtures test dispatch, not biometric accuracy or an actual locked UI.
Physical Noctalia/Hyprlock unlock must be confirmed by the owner on the target
system. Hyprlock need not be installed to test its dedicated PAM adapter.
The same [biometric qualification limits](kde-login.md#status-rollback-and-checks)
apply: no validated presentation attack detection or representative NIR FAR/FRR.
