# Related projects and adopted features

Reviewed official project documentation on 2026-10-13. This is a feature comparison,
not an independent security audit or performance benchmark. Project support claims
belong to their authors; no comparative FAR/FRR, spoof resistance or speed is inferred.
No third-party source code or model weights were copied for these additions.

| Project | Documented workflow relevant here | Linux Hello decision |
| --- | --- | --- |
| [Howdy](https://github.com/boltgolt/howdy#cli) | Enrollment, list/remove/clear, enable/disable, configuration and camera tests | Add self-account status, pause and removal through both GUI and CLI; retain existing camera/testing controls |
| [Gaze](https://github.com/GunduLabs/gaze#cli-usage) | Named face management, refinement, health diagnostics, desktop integrations and uninstall | Add a metadata-only doctor and account overview; retain named enrollment and explicit per-service setup |
| [dax-auth](https://github.com/daxrpm/dax-auth) | PAM verification, protected vault operations and management CLI | Keep fixed-purpose verification separated from administrative reference management |

## Implemented in this update

- English GUI, recognition guidance, PAM/setup messages, installer, desktop entries
  and documentation; original account labels and stored records are preserved.
- Account overview with explicit **Check system status**; unknown state stays unknown
  until an authorized response is received.
- Self-only **Pause face authentication** without removing the protected reference
  or disabling other accounts' sudo access.
- Confirmed **Remove system reference**, retaining password fallback, other users
  and the separate local encrypted reference.
- `hello account status|pause|remove --confirm` using the same authorized backend.
- `hello doctor` / `hello doctor --json` and a GUI health report without camera
  capture, network traffic, images, embeddings, passwords or authentication authority.
- Tests for account binding, partial failures, rollback, lost acknowledgments,
  sanitized reports and translated recognition guidance.

See [account controls](account-management.md), [installation](install-fedora.md)
and [validation](validation.md) for behavior and executed checks.

## Deliberately retained boundaries

The existing YuNet/SFace CPU pipeline remains in place. Switching to SCRFD/ArcFace,
adding a learned PAD model or accepting additional identity profiles requires
model licensing, infrared evaluation and threshold/attack validation. Models in
another project's README are not evidence of compatibility with this camera.

There is no raw snapshot collector, unchecked enrollment refinement, global
security-level slider, automatic shared-PAM enablement, credential/keyring export,
GNOME extension or unsupported distribution package added by this comparison.
These are separate engineering and validation tasks, not missing checkboxes.
Multiple OS users remain supported through UID-bound profiles; each account has
one protected multi-sample reference. This update improves project usability and
management while keeping the existing authentication controls.
