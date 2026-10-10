# Security policy

Linux Hello 0.1.0-dev is a development preview. No stable or security-supported
authentication release exists. Do not install this project's code into a host PAM
stack. The project cannot currently verify a face, perform evaluated PAD, or enroll
a trusted template. Similarity scores and storage tests are not authentication.

Read [the threat model](docs/threat-model.md) and
[authentication boundaries](docs/authentication-flow.md). Security properties must
be supported by implementation and evidence. Any UI/CLI statement suggesting an
unavailable property is active is a security bug.

## Reporting

There is no established public repository or private security contact in this
initial source delivery. Report a suspected issue privately to the repository owner
through a trusted existing channel. When the owner publishes this project, they
must configure a private reporting channel and update this file before inviting
security reports. Do not invent a contact address or post sensitive exploit
details to a public issue while a private channel is being established.

Include the commit/version, operating system, affected component, expected and
observed behavior, a minimal reproduction using synthetic inputs, and the security
impact. Never attach biometric frames, templates, encryption keys, passwords,
personal identifiers or unredacted logs. No response-time promise or vulnerability
bounty is currently offered.

## Required invariants

- No mock or fallback authentication success in production paths.
- No raw-frame retention by default, telemetry or cloud dependency.
- No secrets/embeddings/frames in logs, D-Bus payloads or reusable result tokens.
- No GUI running as root and no automatic PAM/authselect modification.
- No trusted username, PID or identity supplied solely by an IPC caller.
- Missing model, unavailable PAD, hardware errors and timeouts block biometric
  authentication without removing the administrator's password recovery path.
- Package installation cannot silently enable the daemon or authentication.

CI runs code, dependency and license checks. A passing CI result is not an
independent audit, Fedora certification or physical anti-spoofing evaluation.
Advisory exceptions must be narrow, documented, owned and periodically reviewed;
do not ignore entire classes of warnings merely to make a check pass.
