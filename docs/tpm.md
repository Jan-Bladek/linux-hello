# Optional TPM key protection

Implemented on 2026-10-13 for the separately imported root-owned PAM profile.
The GUI passphrase store and face recognition/PAD pipeline are unchanged.
TPM activation is an explicit administrator operation, separate from the GUI's
three authentication choices. Existing profiles continue using their legacy key
until migrated. No production PAM configuration is changed by these commands.

## Design and limits

The Rust verifier uses Fedora's fixed `/usr/bin/systemd-creds` executable,
validated through the same protected root-owned file boundary as the other
helpers. It encrypts the existing random 32-byte template key with
`--with-key=tpm2`, explicitly selects `/dev/tpmrm0`, and explicitly disables
fixed and signed PCR policies. The credential name includes the format version
and claimed UID (`linux-hello-key-v1-UID`). The existing UID-bound AEAD template
is unchanged. No extra Rust dependency, persistent TPM handle, TPM clear,
hierarchy reset, firmware setting or systemd service is introduced.

This mode protects a copied disk from decryption without the original TPM.
It **does not validate boot state**, protect against root or malicious software
running on the original computer, authenticate camera capture, prevent rollback,
or provide presentation attack detection. PCR policy, Secure Boot, measured boot,
update signing and independent recovery/attack qualification remain future work.
Do not infer Windows Hello-level security from a TPM round trip.

The systemd credential mechanism and key modes are documented in
[systemd 259's manual](https://github.com/systemd/systemd/blob/v259/man/systemd-creds.xml).
`--with-key=tpm2` deliberately forbids automatic selection of a host-only key at
creation. Decryption rejects null-key credentials. Only trusted root writers can
replace the credential; the module is not an attestation verifier for arbitrary
externally supplied systemd credentials.

## Build and explicit migration

Requires Linux, a usable TPM 2.0 resource manager device and TPM-enabled systemd
259-compatible `systemd-creds`. Missing tooling/TPM causes an error, not an
unencrypted replacement. The tested host has systemd 259.9 on Fedora 44.
Build the optional helper with the existing feature:

```sh
cargo build --release -p hello-sudo --features sudo-face
```

Install through the existing integration packaging, preserving the KDE-enabled
PAM module when KDE is in use. On an already installed host, updating only the
helper can use an atomic sibling-file replacement followed by `restorecon`.
A helper build alone is not an instruction to replace the PAM module with a
sudo-only feature build.

For Fedora KDE login, install the separate minimal policy first:

```sh
sudo bash packaging/tpm/install-policy.sh
```

This optional `linuxhello_tpm` policy grants only `open/read/write` on
`tpm_device_t` character devices to `xdm_t`. That label includes TPM devices,
not just one pathname. It adds no profile/key writes, unrestricted domain,
network access or permissive mode. The existing camera policy is unchanged.
The ordinary installer does not add this policy on non-TPM installations.
Removing it with `sudo semodule -r linuxhello_tpm` while a profile remains sealed
will make KDE face authentication fail to the password path.

Before migration, verify compatibility in **each enabled service's SELinux and
caller context**. A successful administrator test does not establish display
manager compatibility. Do not migrate an account whose KDE context cannot
release the sealed key. Keep enforcing SELinux and password fallback.

After those checks, an administrator can explicitly migrate an existing normal
account (example UID 1000):

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-enable 1000
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo tpm-status 1000
```

No face re-enrollment or reference passphrase is needed: migration wraps the
existing key. The commands are root-only; ordinary authentication always invokes
`verify`, never `tpm-status` or `tpm-enable`. Status validates the key and encrypted
template and prints metadata only. It is not a biometric authentication result.

Migration shares the GUI setup lock, validates the current configuration and
authenticated template, writes only sealed ciphertext to a private pending file,
unseals it and authenticates the existing template, then atomically publishes
`key.tpm`, fsyncs the directory, removes `key` and fsyncs again. If interrupted
after publishing but before removing the plaintext key, a repeat migration
validates the TPM-backed template and finishes removal. A stale pending file
requires administrator inspection; it is not blindly overwritten.

Presence of `key.tpm` selects TPM even if the file is corrupt, inaccessible or a
broken symlink. It never silently falls back to `key`. Failures propagate through
the existing fail-closed verifier to the original password path. Each credential
subprocess has bounded output, an eight-second timeout, reap-on-error and parent
death handling. Decrypted keys remain in zeroizing Rust memory/private pipes;
no plaintext key, image or embedding is printed. This does not guarantee locked
memory, swap exclusion or zeroization inside systemd's process.

Updating a face on an already migrated profile reuses the sealed key and never
recreates a plaintext key. GUI setup rollback snapshots now include `key.tpm`.
Snapshots of an older plaintext key, filesystem history and backups are **not**
erased by migration, and could defeat the offline protection if recovered.
There is no downgrade/anti-rollback counter or destructive rollback command.

After a TPM reset, motherboard replacement or lost sealed credential, password
login remains available. An administrator must deliberately replace/re-provision
the inaccessible profile using the separately encrypted GUI reference or fresh
enrollment. No automatic unsealing bypass, key recovery or TPM reset is offered.

## Tests

Ordinary helper tests include exact key lengths and distinct account names:

```sh
cargo test -p hello-sudo --features sudo-face
```

The real-TPM regression is ignored by default. It uses synthetic embeddings and
an isolated root profile in a fresh private mount namespace, never the enrolled
person's data or production PAM configuration:

```sh
cargo test -p hello-sudo --features sudo-face --no-run
sudo python3 packaging/tpm/test-hardware.py
# Optional, after release helper build and policy installation:
sudo python3 packaging/tpm/test-hardware.py --kde
```

It exercises real seal/unseal, unchanged encrypted template bytes and threshold,
repeat migration, rejection of corrupted templates before migration, rejection
of cross-UID credential reuse, corruption and broken symlinks despite an
available plaintext fallback, missing TPM and a killed/reaped stalled credential process.
The optional KDE check validates key/template release in enforcing `xdm_t` and
with real UID 1000/effective UID root, using a temporary fixed-purpose probe and
a private helper mount; no real face or PAM success is simulated. The test does not claim live facial verification,
PAD, PCR attestation, suspend/resume, reboot or firmware update qualification.
See [validation](validation.md) for executed evidence and context limitations.
