# Linux Hello threat model

Version: 0.1.0-dev. Reviewed for sudo, KDE login and KDE unlock on 2026-10-13.
This describes the implementation boundary, not an independent security audit.

## Current security boundary

Explicit experimental authentication is implemented for sudo, KDE login and
KDE unlock. It uses protected UID templates, pinned models, caller checks and
multi-frame verification. GUI recognition and PAM authentication are distinct;
neither provides validated PAD, hardware attestation or representative NIR FAR/FRR.
The default diagnostic security state machine still refuses SystemAuthentication.
No fixture or model/camera/cleanup failure may become real biometric success.
See [current scope](index.md) and [authentication flow](authentication-flow.md).

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
authentication stack and installed packages are part of the trusted computing
base. Root/kernel compromise cannot be repaired by this application. A compromised
session can observe that user's preview and misuse their ordinary camera access.

The camera provides pixels, not a signed physical-presence assertion. USB IDs,
device names, IR labels, and sysfs paths are diagnostic hints. They cannot prove
that pixels came from the enrolled sensor. Wayland does not itself create a
trusted camera-to-daemon path.

## Trust boundaries and controls

| Boundary / attack | Required control | Preview state / remaining evidence |
|---|---|---|
| Photograph, display replay, mask, injected frames | Independently evaluated PAD required for a qualified release | Experimental PAM works without validated PAD; IR/geometry/motion must not be represented as spoof protection |
| Username spoofing over IPC | Resolve authenticated sender credentials at the system bus; map request context and target account through policy | No identity-bearing or sensitive RPCs exist |
| Impersonated daemon | Restricted system-bus name ownership; clients require that bus and interface/version; reject owner changes mid-request | Packaged name restricted to dedicated service account; session mode diagnostic only |
| Replayed verification result | Request-scoped deadline, requester unique name, target UID, service binding, one-shot consumption; cancel on disconnect | No verification result/token API exists |
| Unauthorized enrollment/deletion | Explicit account scope and administrator authorization for root import; private storage | GUI edits affect its own reference only; root-owned PAM copy requires privileged import |
| Privilege escalation from GUI/model/parser | GUI unprivileged; no shell evaluation of inputs; privileged surface minimal; model worker isolated | GUI has no system authority; fixed root verifier and self-only setuid launcher are privileged surfaces |
| Template theft | AEAD, authenticated metadata, private files, separation of key from data, no template export | GUI passphrase-derived key; separate root-only PAM key/template; no TPM or offline key separation |
| Symlink/hardlink/path traversal race | Trusted parent directory, ownership/mode checks, nofollow descriptor access, exclusive creation, atomic writes | Storage safeguards require review; never use a writable shared directory |
| Crash during write/delete | Atomic replacement and fsync; versioned records; no successful enrollment before durability | Atomic encrypted persistence and regression tests; no SSD secure-erasure claim |
| Device substitution / hot unplug | Record descriptor identity, reject unexpected formats, timeout and release, re-probe after reconnect | Camera errors reject; descriptor binding is not cryptographic substitution resistance |
| Daemon/camera resource exhaustion | Bounded inputs, work queues, global camera lease, deadlines, rate limits; no unlimited frames over D-Bus | Daemon performs no camera work; capture is explicitly initiated |
| Sensitive logs or crash dumps | Never log images, embeddings, secrets, matching payloads; disable core dumps in service | Sanitized diagnostics only; user-controlled debugger remains outside boundary |
| PAM control-flag error | VM tests of every supported stack, negative cases and password recovery before opt-in | Explicit service-specific activation and rollback; automatic/RPM activation absent; broad VM matrix still pending |
| Malicious model/library update | Pin versions/checksums; verify license and provenance; audit runtime and full dependency tree | Explicit downloader verifies pins; no weights bundled; root verifier refuses user-selected models |
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

Camera frames live only for a bounded diagnostic, enrollment or authentication
operation and
are not saved by default. Releasing the capture handle and UI image after use
reduces retention; memory zeroization cannot erase camera buffers, GPU copies,
swap, filesystem snapshots, backups, or prior crash dumps. Do not claim otherwise.

The template/envelope binds version, UID and model/sensor context. Future
qualified formats must additionally review complete pipeline identity. A template
must bind format version, UID, model identity,
preprocessing identity, dimensionality and sensor requirements as authenticated
metadata. Reject unknown schema/model IDs, NaN/infinity, oversized input, malformed
records and wrong keys. Use an established AEAD implementation and fresh random
nonces, never home-grown cryptography. A similarity score is a model-specific
measurement, not a calibrated probability or standalone authorization.

GUI ciphertext uses a separate user passphrase-derived key. PAM ciphertext and
its random key live together in a root-private profile: this prevents ordinary
users from reading/modifying them but does not protect against root or an offline
disk attacker. Root verification checks fixed, non-writable root-owned ancestors;
store paths, models and target identity are not selected by GUI input. The locker
launcher takes no arguments and derives the only target from kernel real UID.
It executes only the fixed verifier and exposes no enrollment or export API.

Evaluate root-only service credentials or TPM-sealed keys, including backup,
rotation, reinstall, firmware updates, recovery and device theft. A TPM seal can
help protect keys at rest; it does not establish PAD, trustworthy camera pixels,
or human presence. This release has no hardware-backed security claim.

## Password fallback and failure policy

Fail closed means Linux Hello never returns biometric success on failure. It does
not mean removing the existing password path. The supported PAM stacks allow the
administrator's established password policy to run when biometrics are unavailable,
while retaining account, lockout, password-expiration and session checks. Whether
a PAM return code permits fallback depends on control flags and surrounding stack.
The documented narrow stacks have host/isolated dispatch and password fallback
checks; a broader disposable-VM matrix remains required. No universal PAM line
is supplied for unsupported services.

Ordinary installation must not write `/etc/pam.d`, select an authselect profile,
change disk encryption, grant global video access, enable a service, or weaken
SELinux. Explicit experimental activation follows live checks and discloses its limits.
This is not production security qualification: PAD, model/sensor evaluation,
privilege review and deployment/recovery validation remain required.

## Required evidence before a security-qualified authentication release

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
