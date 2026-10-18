# Documentation guide

Current implementation summary, reviewed against the repository on 2026-10-17.
This is a development release with explicit experimental authentication, not a
security-qualified biometric product.

## Scope and terminology

| Term | Meaning |
| --- | --- |
| Default diagnostics | CLI/GUI camera checks and read-only D-Bus; no OS authentication authority |
| GUI reference | One named local face template, optionally encrypted with a separate passphrase |
| PAM reference | A separately imported, encrypted root-owned UID-bound template; local-only edits do not update it; authorized unified setup does |
| Verification | Three fresh suitable frames compared only with the claimed account's reference; no normal head-turn challenge |
| Enrollment | Eight accepted frames with quality/consistency checks; new scans produce V2 templates |
| Experimental authentication | Explicit opt-in for sudo, desktop sign-in or supported screen unlock; errors fall through to the original password path |
| Qualified security | Not achieved: representative NIR FAR/FRR, validated PAD, independent review and deployment qualification remain outstanding |

| Integration | PAM services | Caller binding | Manual activation |
| --- | --- | --- | --- |
| sudo | `sudo`, `sudo-i` | Claimed UID matches caller; trusted PAM_RUSER when sudo's real UID is root | `enroll-and-enable.sh` |
| KDE login | `plasmalogin`, `sddm` | Root display-manager helper and protected per-account/service opt-in | `activate-kde.sh` |
| KDE unlock | `kde` | Claimed UID equals kernel real caller UID; self-only setuid launcher | `activate-lock.sh` |
| Noctalia 5.3.0 / Hyprlock | `login` / `hyprlock` | Claimed UID equals normal real caller UID; independent root-private opt-in | `activate-desktop.py` |

The current pipeline uses YuNet + SFace + OpenCV DNN, not SCRFD/ArcFace/ONNX
Runtime. Five landmarks are not depth or validated anti-spoofing. The threshold
floor 0.80 is provisional. Complete eight-sample V1/V2 templates are compatible;
models from incompatible pipelines must not be mixed.

The normal full-GUI path is scan → name → select uses → **Save and set up**
with a separate reference passphrase and system authorization. The table lists
the retained manual alternatives. Developer controls are hidden by default.

## Reading paths

- [Account controls and health](account-management.md): self-only status, pause, removal and sanitized support reports.
- [Related-project comparison](project-comparison.md): reviewed workflows and adopted features.

- [Complete Hyprland + Noctalia installer](install-hyprland-noctalia.md): full application installation and explicit ordered setup.
- [Full Fedora installer](install-fedora.md): complete source installation and GUI activation.
- [Installation](installation.md): default builds, diagnostics/RPMs and links to optional integrations.
- [Inference](inference.md): exact model hashes, preprocessing, quality, lifecycle and GUI usage.
- [Sudo integration](sudo-integration.md): protected provisioning, authentication, diagnostics and rollback.
- [TPM protection](tpm.md): required wrapping for new system profiles, explicit legacy migration, limits and hardware tests.
- [Hyprland, Noctalia and Hyprlock](hyprland-noctalia.md): session login, screen unlock, activation and rollback.
- [KDE integration](kde-login.md): display-manager login and Win + L, each with separate activation.
- [Architecture](architecture.md) and [authentication flow](authentication-flow.md): implemented authority and data paths.
- [Biometrics](biometrics.md) and [threat model](threat-model.md): template handling, security controls and remaining qualification.
- [Hardware support](hardware-support.md) and [IR qualification](ir-qualification.md): tested camera scope and physical evidence.
- [Model evaluation](model-evaluation.md), [research](research.md) and [dependencies](dependencies.md): dated research, provenance and audit snapshots.
- [Accuracy improvements](accuracy-improvements.md) and [Howdy analysis](howdy-analysis.md): implementation decisions and prior failure analysis.
- [Inference history](inference-history.md): archived, superseded development notes.
- [Validation](validation.md): current evidence summary followed by a historical milestone log.
- [Test entry points](../tests/README.md), [contributing](../CONTRIBUTING.md), [security policy](../SECURITY.md) and [authselect design](../packaging/authselect/README.md).

## Evidence and documentation conventions

Installation means copying files; activation means an explicit reviewed PAM change.
Default/RPM installation never activates authentication. Live checks and an owner
confirmation are functional evidence on the tested Fedora KDE host, not population
accuracy or spoof resistance. SDDM support is implemented, but actual SDDM login
has not been confirmed on this host.

Dated validation entries describe the state at that milestone. Earlier thresholds,
UI steps, disabled authentication statements and failed activation attempts are
historical, not current instructions. Use the current guides above for operations;
retain the dated log for traceability. Dependency audits and research results are
snapshots, not statements that upstream has remained unchanged.

- [Technical audit remediation and open findings](audit-remediation.md)

- [2026-10-17 audit follow-up](audit-remediation-2026-10-17.md): verified code fixes, partial mitigation and open release requirements.
