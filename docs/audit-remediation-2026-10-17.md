# Follow-up to the 2026-10-17 technical audit

The owner supplied a private desktop audit of commit `ff502df`. The report and its
evidence remain local. This is an implementation/status record, not a replacement
audit, new score, PAD certification or measured recognition benchmark.

## Finding status

| Finding | Source change | Remaining scope |
| --- | --- | --- |
| N01: PAD/NIR qualification | Preserve explicit experimental opt-in and unavailable-security disclosures. | Open: no independent PAD engine/evidence, representative calibration or physical attack dataset. |
| N02: PAM parser case/syntax | Rust and Python normalize type/control keyword case, retain module/options case and refuse unsupported continuation, unknown types and malformed records. Shared syntax corpus covers AUTH/Auth/-AUTH, controls, includes and malformed input. Existing managed sudo blocks are revalidated before activation. | Only the reviewed subset is supported; valid unsupported syntax is refused rather than silently ignored. |
| N03: file-backed key | GUI explicitly describes the filesystem/disk access-control boundary. No silent key/TPM migration. | Open: independent at-rest key authority, backup/history and recovery qualification. |
| N04: sudo-i effective policy | Read the actual root-owned `sudo-i` file. Require exactly the reviewed `auth include sudo` alias, then validate sudo and system-auth. Reject extra auth, nested/custom include, cycles, substack substitution and broken override links. | Generic arbitrary PAM graph support is deliberately refused. |
| N05: full pipeline identity | Keep current pinned detector/recognizer and preprocessing. | Open: authenticated full detector/alignment/runtime/scoring identity and explicit template migration. No incompatible templates silently mixed. |
| N06: TPM boot policy | Keep the existing honest no-PCR/no-antirollback limits. | Open: PCR/update/recovery/rollback qualification on separate hardware; no untested changes to the owner's sealed key. |
| N07: interrupted profile journal | Build/fsync a complete `.profile-prepare` before atomically publishing `.profile-update`. Commit/rollback atomically retires the journal before deleting backups. Recovery distinguishes incomplete preparation, active rollback and interrupted cleanup. | Partial: legacy incomplete active journals, full multi-service durable transactions, arbitrary administrator edit CAS and real power-loss/VM coverage remain open. |
| N08: dependencies | Migrate first-party D-Bus code to zbus 6.0.0, integrated object-path types and updated builders; pin required Tokio 1.53.3. | Partial: Slint/windowing still pulls legacy dependencies. Strict audit still fails on five maintenance advisories; no suppression added. |
| N09: admission | Global nonblocking lease plus persistent own-UID admission record, boot identity and monotonic time before key/model/camera work. Reserve 40 seconds for interrupted attempts; successful ordinary cleanup leaves a 3-second cooldown. | Partial: reduces repeated monopolization but is not a fair broker or a population/load measurement. No permanent password/account lockout. |
| N10: install generation | Retain the existing provisioning lock and ordinary per-file installation rollback. | Open: immutable complete generations and a single crash-safe active switch, including PAM/module/worker and SELinux lifecycle, need a separate deployment migration. |
| N11: desktop helper deadlines | Start the GUI deadline before detection. Ordinary-user helpers have output caps, bounded pipe EOF/read/wait and dedicated process-group cleanup; detection/post-config are bounded by 10/30 seconds and the remaining whole-operation deadline. | Kernel uninterruptible I/O and root provisioning recovery have separate limits; the user helper supervisor never kills root provisioning. |
| N12: caller qualification | Restrict Noctalia support to reviewed 5.3.0 instead of accepting every 5.x version. Document stricter probe versus actual account/refresh behavior. | Partial: real locked/expired account/cred-refresh/UI tests, production replay and consent-based end-to-end metrics remain open. |
| N13: secret copies | Disable core dumps and ordinary ptrace dumping in the biometric GUI before creating its UI. Retain existing zeroizing buffers and worker/root dump limits. | Partial: Slint/native allocations, Python snapshots, swap/hibernation and copied backups have no complete wiping guarantee. |
| N14: partial desktop setup | CLI emits explicit PAM/user-config/manual-step status; exit 2 means PAM committed but user overlay pending. GUI reports committed PAM and an overlay-only retry. Retry requires no new root setup or scan. | Multi-manager installer actions remain individually committed, with stop-on-failure and explicit partial-state messaging. |
| N15: private disclosure | Document the repository's visible GitHub private vulnerability reporting form and security page. | Partial: no sensitive or test message sent; owner delivery/triage and response SLA have not been established. |

## Policy semantics and tests

The shared Rust/Python corpus is
[`tests/fixtures/pam-policy-cases.json`](../tests/fixtures/pam-policy-cases.json).
The supported subset rejects line continuations even though Linux-PAM accepts
those logical records. It also rejects unknown `@include` syntax, extra controls
and malformed records. Full-line/inline comments are removed before evaluation.
PAM type/control keywords are case-insensitive; module paths/options retain their
case. This follows the [Linux-PAM syntax reference](https://raw.githubusercontent.com/linux-pam/linux-pam/master/doc/man/pam.conf-syntax.xml)
without pretending to implement every valid PAM arrangement.

An ordinary-user private `pam_start_confdir` test confirms real libpam rejects
mixed-case required deny rules. Fixtures use only system permit/deny modules;
no host PAM, credential, session, camera or real face verdict is involved.
A separate root-only private mount-namespace regression checks the actual runtime
policy reader against a synthetic sudo/sudo-i graph. It must never run without
both namespace separation and synthetic marker checks.

User-helper tests cover a process that never exits, a descendant retaining EOF,
oversized output and an expired whole-operation deadline. Production helpers
still cannot return any biometric authority to the root verifier.

## Recovery and operational changes

The profile phases now have distinct meanings:

- `.profile-prepare`: active files are unchanged; an interrupted partial prepare
  can be cleaned without an index.
- `.profile-update`: complete recovery index/backups are durable before active
  writes; authentication refuses this phase and recovery restores old files.
- `.profile-retired`: publication or rollback was committed; interrupted cleanup
  must keep active files, even if backup/index deletion already started.

All journals remain root-private. The existing recovery command handles the new
phases without starting a camera or editing PAM:

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo recover "$(id -u)"
```

An old pre-fix `.profile-update` missing its index is still refused for manual
review: deleting it blindly could admit an unknown partially written profile.
The improved single-profile journal does not solve multi-service setup or full
bundle crash recovery. Do not infer those properties from ordinary rollback tests.

Admission state is root-private `.admission.json`, separate from templates/keys.
Malformed state fails to password fallback, a new boot resets its monotonic epoch,
and cleanup never overwrites a detected administrator edit. All services for the
same UID share the cooldown; other UIDs have separate records and may compete for
the global lease during the cooldown. No queue grows and password fallback remains
available. This is bounded admission, not proof of fairness under adversarial load.

No model, threshold, template schema or key-provider policy changed, so this
revision requires no biometric re-enrollment. Install the complete matching bundle;
do not copy a default/sudo-only PAM build over the desktop-enabled module.
Existing unknown/custom sudo-i policies now intentionally fall through to password.

## Evidence scope

[Machine-readable evidence](audit-followup-validation.json) records code hashes
and per-command counts for this follow-up: default workspace 80 passed / 5 ignored;
optional selected desktop/biometric crates 130 passed / 13 ignored; Python 65
passed; actual isolated D-Bus tests 2 passed. Five normally ignored root tests were
explicitly executed with synthetic private state/PAM, including current release
worker credential/model loading. Remaining ignored entries were not counted as
executed. Full-workspace desktop-feature Clippy with warnings denied, formatting,
documentation links and license/source/bans checks passed. Strict advisory checking
failed on the five recorded maintenance advisories, with zero known vulnerability
entries in that database snapshot.

Installed the full matching bundle offline with the existing TPM policy. Installed
binaries/PAM/changed management scripts matched source-build checksums; SELinux
remained Enforcing. Current PAM/SUID dispatch tests passed in a private namespace
with simulated verifier exits. The owner's existing protected reference and TPM
provider validated, and previous sudo/login/unlock opt-ins remained enabled.
No production PAM configuration, model, threshold, template schema or key policy
was changed. No new camera enrollment or owner biometric acceptance test was used
to substantiate these code fixes.

Unit/fixture success is not actual owner recognition, PAD/FAR/FRR, tested TPM boot
policy, power-loss recovery or qualification of every supported desktop caller.
The follow-up keeps those release requirements open rather than inventing results.
