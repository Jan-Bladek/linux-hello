# Security policy

Linux Hello 0.1.0-dev is a development release. Explicit experimental sudo,
KDE login and KDE unlock integrations exist, but no stable security-supported
authentication release or Windows Hello-equivalent security is claimed.
Recognition and protected template import work; validated PAD and representative
NIR FAR/FRR remain unavailable. Preserve the original password/recovery paths.
Default installation never enables PAM. See [scope](docs/index.md).

Read [the threat model](docs/threat-model.md) and
[authentication boundaries](docs/authentication-flow.md). Security properties must
be supported by implementation and evidence. Any UI/CLI statement suggesting an
unavailable property is active is a security bug.

## Reporting

The public repository is [Jan-Bladek/linux-hello](https://github.com/Jan-Bladek/linux-hello).
No private security contact or response-time commitment is documented. Report
suspected issues privately to the owner through an existing trusted channel.
Do not post secrets or sensitive exploit details to public issues. A verified
private reporting channel must be documented before inviting vulnerability reports.

Include the commit/version, operating system, affected component, expected and
observed behavior, a minimal reproduction using synthetic inputs, and the security
impact. Never attach biometric frames, templates, encryption keys, passwords,
personal identifiers or unredacted logs. No response-time promise or vulnerability
bounty is currently offered.

## Required invariants

- No mock or fallback authentication success in production paths.
- No raw-frame retention by default, telemetry or cloud dependency.
- No secrets/embeddings/frames in logs, D-Bus payloads or reusable result tokens;
  embeddings use only bounded private worker pipes.
- No GUI running as root and no automatic PAM/authselect modification.
- No trusted username, PID or identity supplied solely by an IPC caller.
- Missing/corrupt models/templates, hardware errors, timeouts, mismatches and
  cleanup failures cannot return biometric success. Original password fallback remains.
- Experimental opt-in must disclose unavailable validated PAD; diagnostic flags
  must never imply qualified security or spoof resistance.
- Package installation cannot silently enable the daemon or authentication.

CI runs code, dependency and license checks. A passing CI result is not an
independent audit, Fedora certification or physical anti-spoofing evaluation.
Advisory exceptions must be narrow, documented, owned and periodically reviewed;
do not ignore entire classes of warnings merely to make a check pass.
