# Linux Hello threat model

Version: 0.1.0-dev. Written before the daemon and PAM integration. Review date:
2026-10-10. This is a design and implementation boundary, not a security audit.

## Current security boundary

This development preview cannot authenticate a person. It has no approved face
model, evaluated presentation attack detection (PAD), trusted enrollment ceremony,
or usable PAM authentication implementation. Diagnostic camera capture, numerical
matching and encrypted-storage tests do not establish those properties. Disabled
operations must return an explicit error; neither a fixture nor a similarity score
may become an authentication success.

The daemon is a read-only diagnostic service. Its interface publishes public
version and security-capability information only. It accepts no username, frame,
template, key, enrollment request, or authentication request. It needs no root
identity, camera device, writable persistent directory, or network socket. A session
bus instance is useful for development but is never trusted for authentication.

## Assets, adversaries, and assumptions

Assets are authorization decisions, account identity, password recovery, biometric
templates, keys, camera frames, service availability, package integrity, and honest
security indicators. A template is sensitive, persistent biometric data: revoking a
file does not revoke a person's face.

Consider an unauthenticated person at the laptop; a local unprivileged process;
another logged-in account; a compromised user session; a process replaying old
messages; a substituted USB or virtual camera; a malicious model; and an attacker
with a stolen disk. Root, the kernel, firmware, the D-Bus broker, the configured
authentication stack and installed packages are part of a future trusted computing
base. Root/kernel compromise cannot be repaired by this application. A compromised
session can observe that user's preview and misuse their ordinary camera access.

The camera provides pixels, not a signed physical-presence assertion. USB IDs,
device names, IR labels, and sysfs paths are diagnostic hints. They cannot prove
that pixels came from the enrolled sensor. Wayland does not itself create a
trusted camera-to-daemon path.

## Trust boundaries and controls

| Boundary / attack | Required control | Preview state / remaining evidence |
|---|---|---|
| Photograph, display replay, mask, injected frames, single IR image | Independent PAD using an evaluated sensor/protocol and challenge freshness; reject unavailable PAD | No PAD implementation; all authentication unavailable |
| Username spoofing over IPC | Resolve authenticated sender credentials at the system bus; map request context and target account through policy | No identity-bearing or sensitive RPCs exist |
| Impersonated daemon | Restricted system-bus name ownership; clients require that bus and interface/version; reject owner changes mid-request | Packaged name restricted to dedicated service account; session mode diagnostic only |
| Replayed verification result | Request-scoped deadline, requester unique name, target UID, service binding, one-shot consumption; cancel on disconnect | No verification result/token API exists |
| Unauthorized enrollment/deletion | Fresh trusted password/admin authentication, verified active subject, explicit scope, no cached authorization for enrollment | No user enrollment/deletion workflow exposed |
| Privilege escalation from GUI/model/parser | GUI unprivileged; no shell evaluation of inputs; privileged surface minimal; model worker isolated | GUI and inference experiments have no authentication authority |
| Template theft | AEAD, authenticated metadata, private files, separation of key from data, no template export | Storage library is developmental; key lifecycle and daemon store unavailable |
| Symlink/hardlink/path traversal race | Trusted parent directory, ownership/mode checks, nofollow descriptor access, exclusive creation, atomic writes | Storage safeguards require review; never use a writable shared directory |
| Crash during write/delete | Atomic replacement and fsync; versioned records; no successful enrollment before durability | Library tests only; no SSD secure-erasure claim |
| Device substitution / hot unplug | Record descriptor identity, reject unexpected formats, timeout and release, re-probe after reconnect | V4L2 errors are diagnostics; substitution resistance unproved |
| Daemon/camera resource exhaustion | Bounded inputs, work queues, global camera lease, deadlines, rate limits; no unlimited frames over D-Bus | Daemon performs no camera work; capture is explicitly initiated |
| Sensitive logs or crash dumps | Never log images, embeddings, secrets, matching payloads; disable core dumps in service | Sanitized diagnostics only; user-controlled debugger remains outside boundary |
| PAM control-flag error | VM tests of every supported stack, negative cases and password recovery before opt-in | No PAM configuration or automatic enablement is shipped |
| Malicious model/library update | Pin versions/checksums; verify license and provenance; audit runtime and full dependency tree | No model downloaded/bundled; dependency checks in CI |
| Offline disk attack | Independent key protection and validated measured-boot policy if TPM used | TPM protection not implemented; encryption alone with colocated key is insufficient |
| False security indicator | Capability states derive from actual functionality; unavailable is never green/enabled | Required invariant for GUI, CLI and D-Bus |

## Future authenticated IPC protocol requirements

System-bus authentication identifies a connection, not the human requesting sudo.
A client-supplied PID, username, PAM service, TTY or action is untrusted. The daemon
must resolve the unique sender to bus credentials and bind authorization to that
connection. A PID alone is subject to reuse. Do not turn a claimed `sudo` string
into authority or trust a D-Bus method merely because its name contains `Verify`.

A future PAM caller must be checked against the operating context and explicitly
supported PAM service. Root caller identity is necessary in some stacks but does
not alone identify which user should be authenticated. Handle account mapping,
real/effective UID distinctions, privilege-changing tools, display managers, and
session state in independently reviewed integration code. Never implement a
universal root-password bypass token.

Every future operation needs a cryptographically random request identifier,
monotonic deadline, bounded input, requester binding and explicit terminal state.
Identifiers locate state and are not bearer authorization credentials. Only the
request's original authorized caller can cancel or consume its result. Result
consumption is one-shot. Disconnect, daemon restart, owner change, timeout,
authorization loss, camera change, malformed output, inference failure and PAD
failure invalidate the request. Reject duplicate transitions and concurrent camera
ownership. Authorize again immediately before durable enrollment changes.

Polkit authorization must target the authenticated system-bus name. Enrollment
requires a fresh trusted prompt and separately selected target UID. A positive
policy decision is not biometric verification. Fail if no suitable agent exists;
do not fall back to a GUI password field owned by Linux Hello. The proposed policy
file denies every action in this preview and is not installed by the RPM.

## Data lifecycle

Camera frames live only for an explicitly started diagnostic capture/preview and
are not saved by default. Releasing the capture handle and UI image after use
reduces retention; memory zeroization cannot erase camera buffers, GPU copies,
swap, filesystem snapshots, backups, or prior crash dumps. Do not claim otherwise.

A future enrollment template must bind format version, UID, model identity,
preprocessing identity, dimensionality and sensor requirements as authenticated
metadata. Reject unknown schema/model IDs, NaN/infinity, oversized input, malformed
records and wrong keys. Use an established AEAD implementation and fresh random
nonces, never home-grown cryptography. A similarity score is a model-specific
measurement, not a calibrated probability or standalone authorization.

A daemon key must not live in the same user-readable directory as ciphertext.
The Linux storage helper rejects symlinks along the path and checks the final
directory's ownership/private mode. It does not establish a privileged deployment's
trust in every ancestor directory. A future root service must use a fixed,
root-owned ancestry that unprivileged users cannot rename or replace, and review
that condition independently. Never accept a root store path from a GUI client.

Evaluate root-only service credentials or TPM-sealed keys, including backup,
rotation, reinstall, firmware updates, recovery and device theft. A TPM seal can
help protect keys at rest; it does not establish PAD, trustworthy camera pixels,
or human presence. This release has no hardware-backed security claim.

## Password fallback and failure policy

Fail closed means Linux Hello never returns biometric success on failure. It does
not mean removing the existing password path. A future PAM stack must allow the
administrator's established password policy to run when biometrics are unavailable,
while retaining account, lockout, password-expiration and session checks. Whether
a PAM return code permits fallback depends on control flags and surrounding stack.
Those combinations require VM tests, not a universal copy-and-paste PAM line.

Ordinary installation must not write `/etc/pam.d`, select an authselect profile,
change disk encryption, grant global video access, enable a service, or weaken
SELinux. Administrator opt-in is necessary but is not sufficient: hardware, PAD,
IPC review, recovery and a supported stack must be validated first.

## Required evidence before authentication can be enabled

1. Independently reviewed code and protocol, authenticated enrollment, evaluated
   model/license/provenance, and attack testing of the complete sensor/PAD path.
2. Measured FAR/FRR and PAD results with documented protocols, dataset consent,
   confidence intervals, sample counts and limitations across the intended users.
3. Camera disconnect/reconnect, replay, substitution, multi-user and concurrent
   request tests on physical target hardware.
4. Disposable Fedora VM tests for sudo, each display manager, each screen locker,
   each supported Polkit agent, missing daemon/model/PAD, malicious IPC and update
   rollback. GNOME, KDE and Hyprland compatibility are separate results.
5. Fedora SELinux enforcing-mode installation/removal and RPM upgrade validation,
   dependency/license review, storage/key review and recovery drill.

Automated unit tests are necessary engineering checks. They cannot establish
resistance to a physical presentation attack. No numeric accuracy, FAR or latency
claim is made until measurement artifacts exist.

## Primary references

- [zbus interface sender/header injection](https://docs.rs/zbus/5.19.0/zbus/attr.interface.html)
- [Polkit subject and CheckAuthorization API](https://polkit.pages.freedesktop.org/polkit/eggdbus-interface-org.freedesktop.PolicyKit1.Authority.html)
- [systemd credential design](https://github.com/systemd/systemd/blob/main/docs/CREDENTIALS.md)
- [ONNX Runtime model validation guidance](https://onnxruntime.ai/docs/)

These references describe mechanisms. The trust policy and release gates above
are project requirements and have not been certified by their upstream authors.
