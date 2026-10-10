# Authselect integration design and VM recovery procedure

**Not implemented and not installable in 0.1.0-dev.** This directory deliberately
contains no PAM template, module-enable script, profile selection hook or RPM
scriptlet. No administrator action can substitute for the missing evaluated model,
PAD, trusted enrollment, reviewed IPC and PAM implementation.

## Update-compatible design

Use an authselect custom profile derived from the machine's actual selected base,
preserving every original feature. Do not assume every Fedora machine uses `sssd`,
and do not edit generated `/etc/pam.d/system-auth` or `password-auth` directly.
Copy PAM templates when modifying them; do not pass `--symlink-pam`. Unmodified
metadata or nsswitch templates can use symlinks when that matches the deployment.
See the [upstream authselect manual](https://github.com/authselect/authselect/blob/master/src/man/authselect.8.adoc).

Copied templates do not automatically inherit upstream security fixes. Each
authselect/Fedora update must trigger a three-way review of the old base, new base
and project change, followed by generated-profile comparison and the entire VM
matrix. Record the base profile version and hashes in a future package's manifest.
Treat unresolved differences as a deployment block. The installation tool must
refuse unexpected administrator changes instead of using `--force`.

Do not provide a universal `sufficient` PAM line. The correct control flow depends
on surrounding account/lockout/smartcard/fingerprint/password modules and service
semantics. Retain password, account, expiry, session and lockout behavior, including
when the biometric module returns unavailable, failure or timeout.

## Before any future experiment

Use a disposable Fedora VM with a snapshot and independently verified console
access. Never begin by modifying the host. Record Fedora, kernel, authselect,
Linux-PAM, systemd, Polkit, display-manager and desktop versions. Confirm an
administrator password works through a fresh console login and keep a separate
root console open during testing. Verify boot media / snapshot restoration access.

Read-only inventory commands:

```sh
authselect current
authselect current --raw
authselect check
authselect backup-list
rpm -q authselect pam systemd polkit
```

If `authselect check` reports unmanaged edits or an unexpected configuration, stop
and investigate. Do not overwrite it. Save the raw selected profile/features,
current generated files and symlinks, profile templates, relevant package versions
and a VM snapshot. A file backup must retain ownership, modes, ACLs, SELinux labels
and symlink targets; copy/pasting file content alone is insufficient.

A future reviewed candidate should first be generated with `authselect test` and
compared without selecting it. An administrator's later explicit selection must
request a named authselect backup and record that backup name outside the VM.
There is no selection command here because there is no functional reviewed
candidate to select.

## Future VM acceptance matrix

Test each supported PAM service independently: sudo (fresh timestamp), console
login, GDM, SDDM if supported, each supported GNOME/KDE/Hyprland locker and each
Polkit agent. Hyprland itself does not identify its login manager or locker; name
the exact programs and versions. Test password authentication before and after
every change.

For each service exercise match, mismatch, unavailable PAD/model, unplugged camera,
stalled inference, daemon crash, timeout, cancellation, locked/expired account,
wrong user, concurrent requests, caller disconnect, late/replayed result and
malformed IPC. Confirm biometrics never succeeds on any error; existing password
fallback and account policy still operate as intended. Verify unattended and
remote callers cannot invoke a physical-presence path by claiming a local service.

Test fresh installation, upgrade from the previous package/profile, authselect base
update, administrator customization conflict, package removal, rollback and reboot.
Test with SELinux enforcing and preserve existing fprintd behavior. A successful
sudo test does not validate screen unlock or an authentication agent.

## Rollback after a future explicitly authorized VM test

Use the independent root console. Identify the recorded backup with `authselect
backup-list`; inspect its provenance. Restore that exact named backup using
`authselect backup-restore BACKUP_NAME`, substituting the recorded name. Then run
`authselect check` and compare `authselect current --raw` to the saved original
profile/features. Test a new password login and sudo invocation while keeping the
recovery console open. Only after recovery works should an experimental module be
removed. A previous selected profile alone is not a complete rollback if its
templates or service-specific PAM files were also changed.

If the console login path itself is broken, restore the verified VM snapshot. For
a future host rollout, use the prevalidated rescue process to restore the complete
configuration/metadata backup, including any service-specific files; then repair
SELinux labels as required and test a fresh password login. If no verified recovery
path exists, do not perform the rollout.

The diagnostic RPM shipped here does not modify authselect or PAM. Its removal
requires no authentication rollback and must not restore or overwrite unrelated
administrator authentication configuration.
