# Follow-up to the 2026-10-18 audit

The owner supplied a private review of `6ad489a`. The report and evidence remain
outside the public source tree. This document records implementation changes and
verification limits; it is not another independent audit or security rating.

## Changes and remaining findings

| Finding | Implementation | Remaining qualification |
| --- | --- | --- |
| N01: PAD and NIR | Experimental opt-in, fail-closed decisions and password fallback retained. No fabricated PAD signal or lowered threshold. | Open: independent, fresh, attempt/sensor-bound PAD and representative NIR genuine/impostor/attack data. |
| N03: adjacent file key | New system profiles require TPM wrapping and a successful unseal before saving. Failure creates no plaintext fallback. New/migrated configurations declare a sticky TPM provider, so a missing blob cannot revive a leftover file key. Existing providers are never silently replaced. | Legacy file-key profiles, historical backups and explicit migration remain a filesystem/disk access-control boundary. Unlink is not secure erase. |
| N05: pipeline identity | Full, versioned embedding namespace and camera binding described below; incompatible references fail before capture. | Source identity does not certify accuracy, sensor authenticity or a runtime's suitability. |
| N06: boot and rollback | No automatic change to active TPM policy. New wrapping preserves TPM-only creation and refuse-null decryption. | Open: PCR/measured or signed boot, supported update/recovery and monotonic anti-rollback policy require separate design and hardware qualification. |
| N07: multi-service interruption | Durable private preimages, prepared/active/retired phases, explicit intents, mode/content/absence checks and root recovery for GUI setup/pause/remove. Ordinary PAM refuses an active operation. | Unrecorded subprocess changes require explicit administrator review. This is not an atomic generation spanning every component or a power-loss-qualified deployment. |
| N09: fairness | Existing bounded global attempt lease and per-UID persistent cooldown retained. Installer now also excludes active attempts. | Open: no fair multi-UID broker or measured maximum biometric wait under load. Password fallback remains available immediately. |
| N10: mixed installation | Durable bundle journal, global attempt exclusion, missing-anchor gate before replacement, epoch revocation, scoped policy checks and anchor publication last. Interrupted installs require recovery. | File replacement is still sequential, not an immutable-generation switch. Real reboot, SELinux-policy interruption and deployment qualification in a disposable VM remain open. |
| N12: actual caller/acquisition | PAM probe now overlays a whole private state root, so production journals stay effective without a diagnostic bypass. Existing production passive replay regressions retained. | Open: offline evaluator still differs from production acquisition; real Noctalia credential/account/cancellation and physical locked-UI matrix are not qualified. |
| N13: retention | Owned contiguous OpenCV pixel/alignment/feature/landmark matrices are wiped on scope exit, including error paths. Outer operations no longer hold every key/profile snapshot in a long-lived Python dictionary. | Slint allocations, Python copies, native DNN internals, swap/hibernation and global erasure remain outside the guarantee. Durable legacy-key preimages are sensitive root-private data. |

## Pipeline compatibility and re-enrollment

`hello-face::pipeline_identity` records both pinned artifact hashes, the supported
color/monochrome conversion and detector-only CLAHE contract, alignment, quality,
scoring/acquisition revisions, input shape and embedding dimensions. It binds
OpenCV version, native build information and CPU-dispatch profile, plus the Rust
compiler/target/optimization flags. Diagnostics expose hashes, not compiler flags.

The implementation digest additionally covers the adapter, normalization,
geometry, sampling and comparison source. This is conservative: even a comment or
test edit in those modules can require re-enrollment. Compatibility is never
inferred solely from an unchanged SFace artifact. This identity is not a claim
that any pipeline has passed statistical or PAD qualification.

The existing encrypted embedding `model_digest` field now carries that full
namespace. Its historical name and the V1/V2 template serialization remain
readable. Camera/IR binding uses a new domain and includes the pipeline digest;
the binding additionally hashes raw camera conversion/driver source, the live
quality/retry/worker bridge and dependency manifests. Thus legal capture-policy
changes cannot silently retain the old camera binding. Legacy references are
**not** silently rebound. The worker protocol is
`LHRDY004`, rejecting old worker executables before capture.

**Re-enrollment is required after installing this update.** Use the complete
matching GUI/CLI/worker bundle, create a fresh local face reference, then explicitly
configure its selected system uses. Existing passwords remain the recovery path.
Debug/release builds and changed native runtime/build profiles may intentionally
be incompatible. Do not copy JSON IDs into an old reference to bypass migration.

## Durable account operations

`operation-journal.py` creates root-private preimage files, flushes them and a
bounded index, then publishes `.operation-update` by directory rename and parent
fsync. No managed target changes during the unpublished prepare phase. Known
postimages are recorded before publication; unknown external-command output is
not guessed during rollback. A retired journal denotes a committed operation and
can only be cleaned, never replayed as an old-state rollback.

Rollback checks every target first, including its mode and absence. Content,
mode, deletion or an unknown postimage causes refusal with preimages retained.
Direct manager rollback also checks mode and unexpected absence. These checks
respect administrator edits observed at the checks; they do not create a kernel
transaction against arbitrary concurrent root editors.

Normal verification rejects active account/install journals, including malformed
marker types. Only an authorized provisioning child with the validated inherited
exclusive setup-lock descriptor continues its own work. The private PAM probe
uses a separate state root/anchor/lock in its mount namespace, not a production
control bypass. Only temporary test copies receive their existing state label.

If recovery is needed, use an administrator terminal and inspect the cause first:

1. Recover an interrupted installation first if its anchor is absent:
   `sudo python3 -B packaging/installer/install-root.py --recover` from reviewed
   matching source. This uses only fixed installation targets and refuses unknown
   policy or file changes.
2. For an active per-profile journal, explicitly run
   `sudo /usr/local/libexec/linux-hello-sudo/hello-sudo recover UID` for the affected
   normal UID. V2 recovery tracks whether a wrapped key predated the operation;
   old V1 journals retain unknown TPM state conservatively.
3. Inspect an account operation with
   `sudo /usr/local/libexec/linux-hello-sudo/recover-operation.py inspect`.
   Output contains previous/planned/current file fingerprints and a review digest,
   never raw keys, templates, names or images.
4. `recover-operation.py restore` performs only recovery with recorded intents.
   Unknown subprocess output requires reviewing the preimages and current changes
   first, then explicitly supplying `restore --expected-state DIGEST`. The digest
   binds both the journal index and current target state. Do not automatically
   pipe an inspection digest into restore or use this to discard administrator edits.

An unpublished prepare directory can be removed through the respective
`recover-operation.py cleanup-prepared` or installer `--cleanup-prepared` action.
These refuse an active journal and do not change managed targets. Sensitive
preimages remain root-private until recovery/cleanup; removing them is not a
promise of physical erasure from disk or backups.

## Installation interruption

The source installer holds both the setup lock and the existing global attempt
lease. It refuses overlap with a pending account operation. Before replacing
bundle files it removes and flushes the anchor already required by old verifiers,
then changes the revocation epoch. New verifiers recheck the anchor after
acquiring the attempt lease, closing the startup-check/lease race. The anchor is
restored only after files,
SELinux policy checks and labels succeed. Recovery retains the blocking journal
and missing anchor until file/policy restoration is complete. The epoch is never
rolled back. This prevents new attempts from treating a partial update as ready;
it is not an immutable-generation deployment architecture or a guarantee for
already mapped old binaries/arbitrary noncooperating readers during an upgrade.

The TPM access policy is included by default; the historical `--with-tpm` flag
remains accepted. Installing policy does not migrate existing keys, change PCRs,
reset the TPM or activate PAM. New references fail if no usable TPM provider is
available. Existing file-key references require explicit migration and review of
historical copies. The source-archive builder excludes the owner's private root
`AUDIT_REPORT.md`.

## Verification scope

The regression suite uses synthetic data and private temporary directories:

- every identity component changes the digest and makes otherwise identical
  embeddings incompatible; old capture bindings and worker protocol fail closed;
- controlled native pixel/feature buffers are zeroed; this is not a global memory
  scan or proof of erasing OpenCV/Slint internals;
- fresh key publication requires successful mocked unseal and never creates a
  plaintext key file; real TPM reset/boot/recovery is not inferred from this test;
- subprocess death between journal writes and bundle replacements, preparation
  fsync failures, corrupt/hardlinked preimages, unexpected content/mode/deletion,
  reviewed-state changes and interrupted committed cleanup;
- actual native model construction/readiness in the bounded worker, with no
  camera frame supplied or biometric acceptance measured.

Executed counts and root/hardware limits are recorded in
[validation](validation.md#audit-follow-up--2026-10-18). No physical spoofing,
FAR/FRR/APCER/BPCER, multi-account fairness or production locked-UI result is
invented. No production PAM configuration or personal reference is modified by
these source changes and synthetic tests.
