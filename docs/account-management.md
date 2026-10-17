# Account controls and installation health

The full application supports protected self-account status, pause and removal
from Overview/Settings and the CLI. These operations require system authorization;
no caller-supplied username or target UID is accepted. The helper resolves the
subject from trusted Polkit/sudo identity metadata and NSS. Ordinary users cannot
modify another account's face enrollment.

## Status

Select **Check system status** in Overview or **Check status** in Settings:

```sh
hello account status
```

A Polkit dialog authorizes reading the current account's protected metadata.
The response includes reference presence, service opt-ins and key backend
(`file`, `tpm` or `none`). An unqueried GUI state is explicitly unknown; it is not
assumed enabled because installation files exist.

Status validates the key provider and decrypts the protected template locally to
check integrity, including TPM unsealing when selected. It never exports keys or
embeddings, captures frames or performs a live biometric match. A validated record
can still fail recognition if the original camera is unavailable or face quality
is unsuitable. This status is never authentication success. Read [TPM](tpm.md)
for the limitations of key wrapping.

## Pause

Select **Pause face authentication**, or:

```sh
hello account pause
```

This disables sudo, desktop sign-in and all supported screen lockers for the authorized account while
retaining its protected reference and key. The original password path remains
available. Other accounts' opt-ins are preserved; sudo is disabled with the
existing per-UID marker, not a global PAM edit. Pause does not delete the encrypted
local GUI reference.

To resume, load the local GUI reference, select the desired use cards and choose
**Save and set up**. All existing live enrollment/activation checks still run.
There is no unchecked enable or a recognition-score bypass.

## Remove

In Settings, explicitly check **I want to remove my protected system face
reference**, then select **Remove system reference**. The user-facing CLI requires an
explicit flag as well as system authorization:

```sh
hello account remove --confirm
```

The helper first disables this account's uses, then deletes its system template,
configuration and file/TPM key. Other users and the local encrypted GUI reference
remain intact. Reconfigure through the GUI to enroll a protected system reference
again. The original passphrase remains necessary to load a retained local file.

Removal unlinks files; it does not guarantee erasure from backups, SSD history or
snapshots. Deleting a reference is not a system-package uninstall.

Management shares the setup lock with GUI setup, TPM migration and installation.
Expected errors restore captured files/PAM bytes; a lost result pipe during an
operation also triggers rollback. Changes are not a multi-file transaction across
SIGKILL, power loss or concurrent unrelated administrator edits. Refresh status
after a failed/disconnected operation. Root-only snapshots are memory-held and
not exported; they are not guaranteed locked or zeroized Python memory.

## Installation health and support reports

Select **Installation health** in Overview or **Run installation health check**
in Help. The report is also available without root:

```sh
hello doctor
hello doctor --json
```

It checks capture-capable camera inventory without starting streams, installed
helpers/PAM/Polkit, adjacent worker, pinned model integrity when built with face
inference, SELinux enforcement and TPM device presence. It includes security
qualification gaps and does not treat an installation check as proof of recognition
accuracy, liveness, TPM key release or active PAM enrollment.

JSON is intended for support issues. It contains version, checks and fixed metadata;
it excludes user names, host names, hardware serial numbers, frame pixels, embeddings,
passphrases and keys. Check it before sharing and optionally redirect it to a file.
The doctor performs no enrollment, PAM edits, network calls or raw-image snapshots.

Status now validates the protected key/configuration/template and pinned model
hashes without camera capture. It does not confirm PAD, boot integrity or actual
recognition. See [audit remediation](audit-remediation.md).

For Noctalia 5.x or Hyprlock session unlocking, see
[Hyprland and Noctalia activation](hyprland-noctalia.md).
