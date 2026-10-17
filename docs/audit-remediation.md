# Technical audit remediation — 2026-10-14

The local technical audit examined commit `a75f091`. This document tracks source
changes and their limits; it is not a new audit score, certification or PAD report.
The original owner-supplied report remains local and is not included in this commit.

See the [2026-10-17 follow-up](audit-remediation-2026-10-17.md) for corrections to
the original PAM parser, sudo-i policy, recovery phases, helper deadlines and
updated dependency findings. The table below records the earlier milestone.

## Changes implemented

| Finding | Change | Remaining validation or limitation |
|---|---|---|
| F02: PAM factor policy | Preserve the reviewed `pam_env` and `pam_faildelay` prechecks before face authentication. Reject unknown password-auth/system-auth modules/options, MFA, access and lockout arrangements. Check the supported policy again in the root verifier before and after capture. | Only the explicitly reviewed local Fedora policy is supported. This does not implement arbitrary MFA integration. Real instrumented MFA/lockout tests in a disposable VM remain required. |
| F03: premature profile publication | Verify the proposed template in memory and complete camera cleanup before writing active files. Hold the shared provisioning lock. Persist `.profile-update` with encrypted template/configuration preimages before publication; authentication refuses any outstanding marker. Ordinary failure restores the previous files. | New partial journals without a valid index require administrator review. No physical power-loss test was performed. |
| F05: premature KDE opt-in | Test a copied private profile and PAM directory in a private mount namespace, including a separate setup lock. Publish the production marker last, after both successful probes and policy conflict checks. Roll back managed writes on late failure. | New activation requires working mount namespaces and root mount permission. Production SELinux/live-camera activation must still be checked by the owner. |
| F06: unbounded input | A monotonic deadline covers Python setup framing, every Rust framed-password read and required EOF. Terminal passphrase input expires after 60 seconds. | Kernel I/O stalls are outside userspace deadline guarantees. |
| F07: ignored stream cleanup | Explicit fallible `CameraSource::stop()` propagates `VIDIOC_STREAMOFF` failure before completion. Drop remains best effort and releases mappings/descriptor. | Synthetic failure tested; hardware unplug/driver failure still needs a test sensor. |
| F08: stale attempts | Recheck service opt-in and protected profile bytes after capture. Shared read locks prevent mixed reads during publication. A fresh root-owned revocation epoch changes on provisioning/pause/remove/activation, including failed operations, so disable/re-enable cannot resurrect an old in-flight attempt. | Epoch is global: configuration activity may reject another in-flight biometric attempt. Password fallback remains. The decision is linearized at its final check; already completed authorization is not revoked. |
| F09: root inference | Read and hash model bytes first; permanently drop real/effective/saved UID/GID to 65534 and supplementary groups before native parsing. Clear keep-capabilities, restore nondumpability, retain no-new-privileges, seccomp and resource limits. | Reserved identity 65534 is used rather than a newly provisioned service account. Qualified SELinux deployment and fuzzing remain required. |
| F10: concurrent startup | Nonblocking global directory lease precedes TPM/model/camera work. Busy attempts immediately fall back to passwords rather than queueing workers. | Persistent per-user rate/backoff and cgroup qualification are still open; one abusive account can contend for the shared sensor. |
| F13: independent writers | Direct sudo/KDE managers, import and TPM mutation use the provisioning lock. Nested root managers inherit a validated descriptor for the same open file description rather than deadlocking on a second independent lock. | Advisory locks coordinate project writers, not arbitrary root administration. |
| F14: recovery | Profile publication has a durable fail-closed marker and explicit recovery. Python setup/account/installer rollback continues independent steps after failures and masks repeated termination signals during recovery. Direct activation tracks its own writes and avoids overwriting detected administrator edits. | Multi-file PAM/setup/installation transactions still need durable generation/journal recovery for SIGKILL/power loss. Outer snapshot rollback does not fully resolve arbitrary concurrent administrator edits. |
| F15: GUI pipes | Shared nonblocking deadline I/O bounds writes, reads and descendant-held EOF independently of child termination. GUI setup covers authorization, input and response under one deadline; account response has its own deadline. | GUI cannot reliably kill a root process. Root helper cancellation/recovery limits remain separate. |
| F16: misleading status | Root `validate UID` checks the exact Config parser, key provider, authenticated template, strict camera binding and model hashes without camera capture/inference. Status no longer treats file presence as successful enrollment. | This confirms stored-data integrity, not physical recognition, PAD, boot integrity or overall usability. A TPM failure yields an unvalidated reference. |
| F17: GUI enrollment | Move pending samples into a scan transaction. Replace the active face only after successful collection, cleanup and the final cancellation check. Preserve the old face on failure; incomplete samples may resume. | A completed failed scan is discarded and must be repeated. |
| F18: metric scope | Evaluation JSON explicitly names its offline 2–12-frame protocol and states that PAM acquisition/end-to-end authentication metrics were not evaluated. | A shared production replay harness and representative consent-based data are still needed. |
| F20: wildcard gate | Add the explicit workspace version constraint to the helper's hello-cli dependency. | Duplicate dependency versions remain warnings for review. |
| F21: SELinux upgrades | Inspect priority and exported HLL bytes of loaded modules. Keep matching packages, upgrade only when the previous installed package matches the loaded module, reject unknown administrator overrides, and verify/roll back known upgrades. | Actual global policy upgrade/rollback requires a disposable Fedora VM. Unsupported CIL or higher-priority overrides require manual review. |
| F22: partial installers | Retire both legacy partial installers; use the complete manifest-based root installer. They now refuse before making changes. | Full bundle publication is still multiple atomic file replacements, not an atomic generation switch. |
| F25/F26: gates and CI | Correct dependency evidence; remove redundant unpinned Rust workflow and retain the pinned, locked primary workflow. | Online CI results must be inspected independently after publication. |

## Findings still open

- **F01/F23:** No independent PAD implementation/evaluation or representative NIR
  FAR/FRR qualification is available. Experimental face matching is still an
  explicit opt-in; default installation does not activate PAM. Do not treat the
  technical audit as a passed anti-spoofing evaluation.
- **F04:** Legacy root-private plaintext keys provide an ACL boundary, not
  independent protection of copied disk/backup data. TPM wrapping is available;
  migration does not erase historical copies. No unattended migration was added.
- **F11:** Existing template bindings do not encode the entire detector,
  preprocessing, scoring and native-runtime identity. Current pinned models and
  preprocessing are retained to avoid silently invalidating existing references.
  A versioned pipeline migration/re-enrollment remains required before qualification.
- **F12:** TPM wrapping has no PCR/boot-state or anti-rollback policy. Enabling a
  boot policy without testing recovery and kernel updates could lock out references;
  no such policy was invented or silently imposed.
- **F19:** Strict advisory checking failed on two unmaintained transitive
  dependencies at this milestone; the 2026-10-17 follow-up records five. No advisory suppression or incompatible replacement was added.
  `ttf-parser` is reachable through the Linux windowing graph. `bincode` remains
  in the lockfile; the selected all-feature tree prints no reverse dependency.
- **F24:** Rust-owned secret buffers are zeroized where supported, but Slint,
  native OpenCV matrices, Python snapshots, swap and allocator copies do not have
  complete wiping guarantees.
- **F27:** A verified private disclosure channel still needs repository-owner
  configuration. No mailbox, receiving contact or response commitment is fabricated.

## Recovery and upgrade

These are source changes. They do not automatically rewrite this host's production
PAM files, activate services, migrate its TPM key or replace installed binaries.
Build and install the complete bundle through `./install.sh` as the regular user.
Then explicitly reactivate selected integrations to migrate their known managed
PAM blocks and run the live checks. A new verifier deliberately refuses legacy
blocks lacking the preserved prechecks, allowing password fallback until migration.
A recognized old KDE companion is migrated; arbitrary administrator changes are
refused. Existing enrolled faces and model formats remain compatible: routine
re-enrollment is not required for this patch.

If a profile update was interrupted, close the GUI and review the failure, then
run the installed root helper explicitly through sudo:

```sh
sudo /usr/local/libexec/linux-hello-sudo/hello-sudo recover "$(id -u)"
```

Recovery restores only the validated journal's prior encrypted template/configuration,
removes a key created by the interrupted operation, and clears the marker after
syncing. It does not capture a face or change PAM opt-in. A malformed/incomplete
journal fails closed and requires administrator review; do not delete markers to
make an unknown partial profile authenticate.

## Validation

Regression tests use synthetic pipes, profiles and decisions, not personal images.
They cover partial headers/bodies/missing EOF, nonreading peers, inherited EOF,
unknown MFA policy, corrupt-reference status, late KDE conflict, continued rollback,
failed STREAMOFF and staged enrollment. Root tests explicitly require a private
mount namespace and a synthetic marker. They never mount a fake verifier over a
production executable or change the global SELinux policy store.

Run the nonprivileged checks:

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo test --locked -p hello-sudo -p hello-pam -p hello-settings -p hello-cli -p hello-camera \
  --features hello-sudo/sudo-face,hello-pam/kde-pam,hello-settings/face-diagnostics
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -B -m unittest discover -s tests -p 'test_*.py'
bash tests/integration/run-session.sh
python3 tools/check-docs.py
cargo deny --locked check licenses sources bans
cargo audit --deny warnings
```

The strict advisory command is expected to fail until F19 is resolved. Local
passing tests do not qualify actual login/unlock, PAD, population accuracy,
SELinux upgrades, boot recovery or physical power-loss behavior.

The local remediation checks passed: default workspace tests, optional
camera/CLI/GUI/helper/PAM tests, both Clippy configurations, formatting, Python
regressions, documentation links and release compilation. Both private D-Bus
integration tests passed. License/source/bans checking passes with warnings;
strict advisories still fail on F19. Synthetic root recovery and actual release
worker credential/model-loading tests passed in a private mount namespace.
The latter uses the real supervised worker executable, not credential changes
inside the multithreaded Rust test harness; those earlier harness attempts timed out.
No physical accuracy, PAD or power-loss benchmark is reported.

[Recorded checks and limits](audit-validation.json) bind this run to the baseline
and a SHA-256 of the staged source diff. Real PAM dispatch with the newly compiled
module and launcher also passed using simulated verifier outcomes in a private
namespace, including earlier required denial and missing/corrupt opt-in.

## Installed-host follow-up

The owner subsequently requested deployment. The full bundle was installed with
cached dependencies/models and the existing TPM policy. Enrolled data and key
were retained. A provisioning test exposed a nested read-lock issue: the verifier
must duplicate the inherited exclusive open description rather than acquire an
independent shared lock or downgrade its parent's lease. A new synthetic root
regression confirms both successful delegation and continued exclusion of an
independent reader after the child closes its duplicate.

The installed camera/inference path ran, but the live sudo activation acquired
0/3 clear faces and rejected the attempt. This is a failed acquisition, not an
accuracy/PAD benchmark. No PAM configuration migration was committed by that
attempt. Password fallback is available; the stricter verifier refuses old
managed stacks until the owner completes explicit activation while facing the
camera. Installed code is therefore not evidence of completed live activation.

After the owner confirmed camera readiness, the repeat activation succeeded: the
sudo reference matched three fresh frames, both PlasmaLogin probes and both
unprivileged KDE locker probes returned successful auth/account/setcred. The final
self-account status reported all three uses enabled and TPM key/template validation
succeeded. Hash comparison confirmed the existing protected template/configuration
and key were unchanged. SELinux remained Enforcing. No desktop logout, lock,
display-manager restart, new enrollment or TPM migration was performed. These are
functional checks on the existing owner/sensor, not PAD or population qualification.
