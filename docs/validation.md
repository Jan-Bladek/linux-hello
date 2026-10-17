# Executed validation

## Current evidence summary — reviewed 2026-10-17

| Area | Latest recorded evidence | Remaining limitation |
| --- | --- | --- |
| Fedora target | Lenovo Yoga Slim 7 14IMH9, Fedora 44 KDE/Wayland; GREY/IR and YUYV capture, cancellation/reopen and scoped emitter restoration tested | Other devices, spectral validation, suspend/unplug and long-duration coverage incomplete |
| GUI face workflow | Owner-confirmed detection, named enrollment and passive recognition; encrypted V1/V2 storage and synthetic failure regressions | No representative genuine/impostor accuracy study |
| sudo | Real owner match and original password fallback tested; protected UID reference and fixed verifier installed | Explicit experimental opt-in; no validated PAD |
| KDE login | Owner activation minima0.868/0.880 and AUTH/ACCT/SETCRED0; owner subsequently reported real login works | Plasma Login Manager confirmed; actual SDDM login not measured here |
| KDE unlock | Own-UID launcher and real unprivileged password fallback tested; owner subsequently confirmed Win + L works | No independent unlock latency/accuracy/attack report |
| Hyprland / Noctalia | Dedicated self-account `login` / `hyprlock` adapters, protocol/rollback/configuration regressions | Physical lock-screen unlock awaits owner confirmation; initial Hyprland sign-in reuses the existing display manager |
| Software | Recorded default/feature Rust, Python, rendering, worker and privileged isolated-dispatch checks passed at their milestones | Counts below belong to each recorded commit, not one current aggregate |
| Dependencies | 2026-10-17 GUI follow-up: strict Cargo audit passes for 602 dependencies / 1,338 advisories; all five flagged packages removed | Local compatibility patches require maintenance; native-library audit remains open |
| Release qualification | Functional implementation and owner confirmations | Representative NIR FAR/FRR, PAD, independent privilege/key review, broad VM/RPM lifecycle and other desktops remain open |

Current usage and activation are in [scope](index.md), [inference](inference.md),
[sudo](sudo-integration.md), [KDE](kde-login.md) and
[Hyprland / Noctalia](hyprland-noctalia.md). This document preserves evidence;
it is not the installation guide.

## GUI dependency advisory remediation — 2026-10-17

All five maintenance-advisory packages from the initial follow-up were removed
from the lockfile, using the [reviewed compatibility patches](../third-party/README.md).
No advisory ignores or CI gate changes were introduced.

| Check | Actual result |
| --- | --- |
| `cargo audit --deny warnings` | Passed: 602 dependencies, 1,338 advisories; RustSec database `999ec89d5d3f10f20be14750ec8c2d8b44e0ac9b` |
| `cargo deny --locked check licenses sources bans` | Passed; duplicate-version and unused-license-allowance warnings remain |
| Default workspace Rust tests | 81 passed, 6 ignored |
| Optional camera/CLI/PAM/sudo/GUI tests | 131 passed, 14 ignored; ignored physical/privileged cases were not run |
| Isolated session-bus suite | 3 passed, including modern/legacy portal reads with nested variants outside Tokio |
| Default and optional-feature all-target Clippy | Passed with warnings denied |
| Python tests | 65 passed |
| Formatting, documentation links and staged whitespace | Passed |
| Locked workspace release build | Passed |
| Native Wayland GUI smoke | New release window mapped on Hyprland with software rendering and remained running; no face scan started |
| Vendored source packaging | Archive created with both local patches; private audit absent; offline locked metadata verified with an empty Cargo home |

These checks do not measure face accuracy or PAD and do not qualify all desktop
compositors. Native title rendering on other Wayland compositors and distro font
packages remains deployment-specific. No installed PAM/service/profile state was
changed and no re-enrollment is required. The previous follow-up evidence below
remains a historical record of its source revision.

## Follow-up audit fixes — 2026-10-17

The [follow-up tracker](audit-remediation-2026-10-17.md) maps all N01–N15 findings
from the owner's private report of `ff502df` to implemented fixes, partial
mitigations and open requirements. The original report/evidence remain local.
[Recorded checks](audit-followup-validation.json) include actual private libpam
case semantics, root runtime sudo-i policy, staged/retired profile recovery,
persistent admission, ordinary-user helper deadlines and desktop partial-state
reporting. First-party D-Bus code uses zbus 6 with isolated caller/denial regressions.
The complete updated bundle was installed; existing TPM-backed reference and
previous opt-ins validated. No population/PAD, owner face acceptance, physical
power-loss or complete immutable-generation qualification is inferred. Strict
advisories remain a failed release gate, not a suppressed warning.

## Hyprland / Noctalia / Hyprlock support — 2026-10-17

Inspected the running Fedora 44 Hyprland / Noctalia 5.3.0 environment and reviewed
Noctalia source commit `3982122906f2409aeaff17399552153c4c323f20` plus official
Hyprlock documentation. Noctalia 5 uses unprivileged `login` PAM, not the old
Quickshell `system-login` service. Added whitelist-bound own-UID launcher routing,
independent markers and service-specific password companions without enabling
root TTY face login or editing shared authselect stacks. Existing PlasmaLogin
continues to select the Hyprland session. GUI setup selects the running locker;
account pause/remove now revokes every supported locker. The unprivileged
Noctalia overlay permits empty-submit PAM initiation without changing lock/grace
policy and detects later settings overrides.

Executed default workspace tests (79 passed, 5 restricted/hardware entries ignored),
optional camera/CLI/GUI/PAM/helper tests (127 passed, 11 ignored), both Clippy
configurations with warnings denied, formatting and 54 Python tests. Added exact
PAM round-trip/shared-account rollback coverage and Noctalia configuration conflict
regressions. Ignored entries are not physical camera, TPM or biometric evidence.
New locker support does not imply measured FAR/FRR or validated PAD.

Installed the complete updated bundle using the full offline Fedora source
installer with the existing TPM policy. Installed scripts, PAM module and launcher
matched source/build hashes; SELinux remained Enforcing. The actual installed
PAM module and setuid launcher passed private-namespace dispatch tests for both
new services: own-UID success, missing/corrupt independent opt-in rejection,
root TTY rejection, original parent/account/setcred success and verifier mismatch.
Synthetic helper exits tested dispatch only; production PAM/templates were not
replaced by fixtures. Also exercised actual Noctalia 5.3.0 config parsing in a
temporary XDG home: the overlay enabled empty-submit while locking stayed enabled,
without changing running-session settings. Physical owner scans and locked-screen
unlock for these new adapters remain pending; use the activation tool deliberately.

## English UI and self-account management — 2026-10-13

Reviewed official Howdy, Gaze and dax-auth workflows; the dated
[comparison](project-comparison.md) records adopted management/health features
without inferring their accuracy or attack resistance. Added authorized self-only
status, pause and confirmed system-reference removal in GUI/CLI, plus an
unprivileged sanitized installation health report. GUI, recognition/PAM/setup
messages, installer, desktop entries and current documentation now use English.
User labels and stored biometric records are not translated. Earlier UI labels
in this document are rendered in English for readability; their original milestones
could predate the language change. The README credits OpenAI Codex explicitly as AI assistance, not a separate
GitHub user.

Default workspace tests/Clippy and optional GUI/CLI/PAM/helper tests/Clippy passed.
The optional CLI library has 72 passing tests (1 restricted-child entry ignored
in the ordinary runner), GUI 11, PAM 8 and sudo helper 3 (1 real-TPM entry ignored).
The Python suite has 41 passing tests including 4 account-management regressions.
All 28 Markdown files passed link/heading checks; formatting, shell/Python syntax,
desktop-file validation and diff checks passed. English software-renderer snapshots
cover every tab and enrollment state at desktop/minimum sizes; the public screenshot
contains UI only.

Installed full binaries/helpers through the existing atomic source installer.
Explicitly executed real management/child-manager status/pause/remove in a private
mount namespace with synthetic references and an additional opted-in account.
Verified other-account markers and shared PAM bytes unchanged, rejection of a
root/system authorization subject, and exclusion of synthetic secret data from
responses. Ordinary regressions additionally verify partial-failure and lost-ack
rollback. This is management coverage, not a live biometric acceptance test.
The installed actual owner's authorized status reports enrolled, all three uses enabled and
TPM backend; `tpm-status` validates the current key/template. Password fallback,
SELinux Enforcing and existing recognition controls remain in place. A six-second Wayland launch of the installed English GUI completed without errors
(terminated by timeout). Private-namespace PAM dispatch tests also passed for
match/rejection and auth/account/setcred, with simulated helper exits only. Doctor JSON
has 13 checks and explicitly excludes capture, biometric data and authentication
verdicts. CLI removal without confirmation is rejected before authorization.

New account operations have not been exercised through a human-operated Polkit
GUI dialog or a new live face login in this session. Missing consented biometric
benchmarks, reviewed PAD evidence and broader deployment qualification remain open.

## Complete Fedora source installer — 2026-10-13

Added `install.sh` and unprivileged build/staging plus fixed-path administrator
installation. It builds full GUI/CLI recognition, sudo and KDE PAM, pinned models,
probe and policy; it never activates PAM/accounts or starts the diagnostic daemon.
The recommended entry point and first-run guide were initially written in Polish;
the current guide is English.

Executed on the existing Fedora 44 KDE host: dry-run; full offline/skip-deps
build-only with TPM policy; 22 GUI/helper/PAM tests passed (real-TPM test ignored
by default); full release binaries and hardened C probe/policies built; model
hash/size checks passed. A fresh private mount-namespace file-install test passed
with existing policies, root/setuid modes and installed CLI smoke checks. Five
installer regressions cover unsupported distributions, changed files, model pins,
links/path injection, rollback and inert dry-run. The Python suite now has 37
passing tests, including source-archive documentation discovery; 25 Markdown
files passed link checks. Shell syntax, Python compilation, desktop-file validation
and diff checks passed.

The complete installer also ran as the ordinary owner with actual sudo
authorization to update the current host. Before/after byte comparisons confirmed
all checked active PAM files, encrypted template, profile configuration, TPM key
and service opt-in markers unchanged; the plaintext key remains absent.
Installed sudo, PlasmaLogin and KDE unlock statuses remain enabled; TPM status
validates the current key/template; SELinux remains Enforcing. The installed GUI
launched under Wayland for six seconds without error (terminated by timeout),
and CLI help worked. No new live face match or session lock was performed.

The first-install DNF transaction, rustup bootstrap/model downloads on a clean VM,
new policy installation on a clean host, RPM lifecycle, other Fedora releases and
other camera models were not validated by these tests. The installer is a source
installer, not a reviewed/signed binary RPM distribution.

## TPM implementation — 2026-10-13

Added optional Rust integration with systemd 259.9's TPM-only encrypted credentials.
Ordinary helper tests: 3 passed, 1 hardware test ignored by default. Formatting,
feature Clippy with warnings denied and release helper build passed. Python suite:
31 passed. Real TPM test explicitly executed in a fresh private mount namespace
with synthetic templates: migration/unseal, threshold and ciphertext preservation,
idempotence, malformed template, cross-UID name mismatch, corrupt/broken credential,
missing device and an eight-second stalled-process timeout passed. No person's
biometric data is used by the hardware regression.

An initial enforcing `xdm_t` key-only check failed: audit AVCs showed denied
read/write, followed by open, on `tpm_device_t`. The separate optional policy
adds precisely those permissions. Subsequent key/template-only checks passed in
`xdm_t` and with real UID 1000/effective UID root; Enforcing remains active.
This validates key release contexts, not a live face match, reboot, firmware
updates, PCR policy, latency distribution or anti-spoofing.

The rebuilt helper and updated setup coordinator were installed atomically on the
Fedora host. UID 1000's existing key was migrated, the plaintext `key` removed,
and `key.tpm` is root-owned mode 0600. The current real profile's key and encrypted
template were then validated in `xdm_t` and with the account's real UID/effective
root. sudo, PlasmaLogin and KDE unlock opt-ins remain enabled and PAM files were
not modified. A live face login after this migration and a reboot have not been
performed by this implementation session.

The owner reports a positive private PAD audit held by an auditor. No report,
scope, tested version, hardware or methodology was available for review here.
No PAD qualification flag or biometric threshold was changed on that basis.

## Documentation consistency review — 2026-10-13

Reviewed all repository Markdown guides against the current feature gates,
installed integration scripts, CLI help and source boundaries. Updated current
README, architecture, enrollment/authentication flow, security/storage scope,
installation/rollback and test entry points. Superseded inference development
notes were retained in [the archive](inference-history.md), with current usage
in [inference](inference.md) and a central [documentation index](index.md).
Historical measurements below were preserved rather than reinterpreted as current
population/security evidence. Dated dependency snapshots were not presented as
a fresh audit.

The standard-library documentation checker validated all 23 Markdown files' local
file/image links and heading fragments. Its 2 regression tests and the complete
Python suite passed (28 tests). `git diff --check` passed. The same link check was
added to CI. External URLs were not fetched, and no camera, PAM configuration,
biometric reference or runtime code was changed for this documentation review.
No full Rust rebuild or new physical/security qualification is claimed.

## Historical milestone log

All entries below describe their own milestone. Earlier disabled-PAM states,
thresholds, head-turn requirements, acquisition limits and unsuccessful activation
attempts are historical and may be superseded by later entries. Test totals must
not be added together; hardware and synthetic tests establish different things.
Ignored local `work/` paths identify workstation artifacts, not files distributed
with a source checkout. No missing benchmark or attack result is inferred.

## Native Fedora and physical cameras — 2026-10-11

Execution restrictions were removed and the locked dependency graph was fetched.
The host is the Lenovo Yoga Slim 7 14IMH9 (`83CV`, BIOS `NPCN35WW`) running
Fedora 44 KDE/Wayland and kernel `7.2.9-200.fc44.x86_64`. Camera ACLs already
grant the desktop user access; no device permissions or authentication settings
were changed. Administrator privileges were not needed for these checks.

| Check | Result |
| --- | --- |
| Locked full workspace build, formatting and all-target Clippy | Passed |
| Locked full workspace tests | 49 passed; physical camera, physical CLI cancellation and two isolated-bus tests ignored in the ordinary invocation |
| Explicit isolated D-Bus tests | 2 passed |
| Python recorder boundary tests | 5 passed |
| Explicit camera capture/release/reopen | Passed on `/dev/video0` YUYV 640×480 and `/dev/video2` GREY 640×360 |
| Explicit CLI SIGINT cancellation and reopen, after fix | Passed on both capture nodes |
| Full workspace release build and rebuilt release CLI after fix | Passed |
| Release CLI/daemon smoke test on isolated session bus | Passed; clean SIGTERM shutdown |
| Native GUI startup | Survived a five-second Wayland launch with no error output; terminated by the test timeout |
| Packaging shell/XML checks; scratch systemd unit verification | Passed; unit was not installed or started |
| `cargo audit --deny warnings --json` | Failed: zero known vulnerabilities, two unmaintained-dependency advisories |
| `cargo deny --locked check advisories licenses sources bans` | Advisories failed; licenses, sources and bans passed |

Physical testing exposed a CLI cancellation problem: `ctrl_c()` was recreated
for every frame, dropping the receiver whenever the frame branch won the select.
One GREY capture required termination after failing to handle SIGINT. The CLI
now pins a single signal future for the whole capture and prioritizes cancellation.
An explicitly ignored CLI hardware regression test checks SIGINT shutdown within
five seconds and then reopens the same node. The recorder runs it only after an
explicitly selected camera passes the basic capture test. Both nodes passed the
final recorded checks. Earlier failure logs remain available.

One initial RGB diagnostic capture also failed because the driver marked a frame
corrupted. The application rejected that frame and released the camera; subsequent
captures passed. This observation is retained rather than treated as a clean
long-duration stability qualification. No corrupted pixels were accepted, and no
camera frames were saved.

Final reports are `work/fedora-final-rgb-2026-10-11/report.json` and
`work/fedora-final-grey-2026-10-11/report.json`. Release, audit, service and
earlier physical-test records are in `work/fedora-host-rgb-2026-10-11/` and
`work/fedora-host-grey-2026-10-11/`. RustSec database commit remained
`7eebec69c352c7191b1f13eb95dd510eeca5d1de`; advisories are still
RUSTSEC-2025-0141 and RUSTSEC-2026-0192, without suppressions.

This establishes basic native capture, conversion, cancellation and reopen. It
does not establish IR spectrum/emitter control, indicator behavior, shutter
operation, suspend/resume, unplug/replug stability, interactive accessibility,
biometric accuracy, PAD or authentication readiness. RPM lifecycle and privileged
authentication remain untested and unavailable respectively.

## Fedora build after installing prerequisites — 2026-10-11

The host administrator installed the documented development packages. The agent
can now find libclang and the fontconfig, freetype2, xkbcommon and wayland-client
pkg-config modules. The locked debug workspace build passed, producing `hello`,
`hello-daemon` and `hello-settings`.

| Check | Result |
| --- | --- |
| `cargo build --locked --workspace` | Passed, including camera, storage and GUI |
| Offline workspace tests excluding storage and the CLI isolated-bus test | 42 passed; physical camera and two isolated-bus tests ignored; one CLI bus test filtered |
| Offline Clippy, all targets, excluding storage | Passed with `-D warnings` |
| Offline Clippy, all production libraries/binaries, including storage | Passed with `-D warnings` |
| GUI rendering test with an added shared-header invariant | Passed; five tabs at 980×740 and 760×660 |
| Python qualification-recorder tests | 5 passed |
| CLI security, camera enumeration and daemon help | Passed; authentication disabled; camera list empty in sandbox |
| Full workspace tests and all-target Clippy including storage tests | Blocked by uncached `tempfile` 3.27.0 and unavailable crates.io DNS |
| Isolated D-Bus tests | Blocked by sandbox socket/runtime-directory restrictions |
| Physical capture and interactive Wayland GUI | Not executed; camera nodes remain hidden from the sandbox |

The partial test command was
`cargo test --locked --offline --workspace --exclude hello-storage -- --skip missing_daemon_never_returns_successful_status`.
This exclusion is recorded here rather than treated as a full workspace pass.
The GUI test now compares the shared header between tabs as well as checking
dimensions and rendered content. Ten actual UI snapshots were exported under
`work/fedora-installed-ui/`; all five minimum-size pages and the desktop-size
overview were visually inspected. Header pixels are identical across tabs.
This is headless rendering validation, not interactive desktop qualification.

The qualification report and supplemental test/Clippy logs are under
`work/fedora-installed-2026-10-11/`. To finish fetching the locked graph, run
`cargo fetch --locked` in a network-enabled host terminal, then rerun the full
checks. Camera and D-Bus qualification also require a host execution context with
the normal desktop user's access.

## Diagnostic pipeline follow-up — 2026-10-11

The face library now includes a coordinator enforcing model/input/output contracts
between detection, alignment and embedding. Six new tests use a test-only engine
and synthetic pixels; they validate successful stage ordering, early rejection of
invalid model/policy/frame/quality inputs, malformed detections, aligned tensors,
and mismatched model/output dimensions. No operational model was added.

The qualification recorder now continues with library tests and Clippy after a
failed full build. The overall report remains failed/blocked, and camera, storage,
CLI and GUI checks are not represented as passing by this fallback.

| Check | Result |
| --- | --- |
| Locked offline tests: core, face, IPC, fingerprint, PAM, daemon | 19 passed; two isolated-bus tests ignored |
| Locked offline Clippy for those packages, all targets | Passed with `-D warnings` |
| Python recorder boundary tests | 5 passed |
| Workspace formatting and diff whitespace | Passed |
| Full locked offline workspace build | Blocked by missing native development libraries |
| Physical camera, model inference and authentication | Not executed; model inference and authentication remain unavailable |

The local command logs and report are in
`work/fedora-pipeline-2026-10-11/`. These checks extend the diagnostic foundation;
they do not measure biometric accuracy or PAD performance.

## Fedora continuation — 2026-10-11

The project was resumed on Fedora Linux 44 (KDE Plasma, Wayland), x86-64,
kernel `7.2.9-200.fc44.x86_64`, machine type `83CV`, BIOS `NPCN35WW`, with
Rust/Cargo 1.99.0. Commands ran inside the coding agent's restricted environment,
so these results do not constitute physical desktop or camera qualification.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| Offline tests for core, face, IPC, fingerprint and PAM | 13 passed; isolated fprintd test ignored |
| Offline daemon tests | Compiled; isolated-bus test ignored |
| Offline Clippy, all targets, core/face/IPC/fingerprint/PAM/daemon | Passed with `-D warnings` |
| Python qualification-recorder boundary tests | 4 passed; no real camera access |
| Locked full workspace tests | Blocked by uncached `tempfile` 3.27.0; crates.io DNS unavailable |
| Locked offline CLI/daemon build | Camera bindings blocked by missing `libclang` |
| Locked offline workspace build | Blocked by missing fontconfig development library; preflight also found missing freetype2, xkbcommon and wayland-client |
| Isolated fprintd test via `dbus-run-session` | Bus could not start: sandbox denied socket creation and runtime-directory writes |
| Physical capture/reopen and interactive GUI | Not executed |

Sysfs reports four integrated camera nodes (`video0` through `video3`), but none
is exposed under this environment's `/dev`. Their formats and capture roles
remain unqueried. This is distinct from the earlier WSL environment with no
reported camera hardware, and does not establish usable RGB or IR capture.

`tools/qualify-linux.py` now creates a private report directory under ignored
`work/`, records preflight metadata and optional software checks, and requires
an explicit `--device /dev/videoN` for the existing bounded physical test.
The local recorded preflight/build attempt is in
`work/fedora-2026-10-11/report.json`; it contains no frames or serial numbers.
Reports remain local and are not included in source control.

Next, install the [documented build prerequisites](installation.md#fedora-development-build)
through the host's normal package process, make the locked dependencies available,
and run the recorder from the desktop terminal. Enumerate with `hello cameras`
before selecting the physical node. The previous remaining biometric and RPM
release gates below are unchanged.

## Previous milestone — 2026-10-10

This report distinguishes software checks from physical biometric, desktop and
Fedora validation. **This is not a production authentication release.**

## Environment

- Windows x86-64 host; Rust/Cargo 1.99.0, MSVC target for portable checks.
- Ubuntu in WSL2, x86-64 Linux 6.18.33.2-microsoft-standard-WSL2.
- Rust 1.99.0, Linux GNU target; native C/Clang, font and Wayland development
  libraries installed in WSL for compilation. The toolchain/cache/build products
  were kept outside the delivered repository.
- No V4L2 video nodes were available: the release CLI returned `[]` from `cameras`.
- No host PAM/authselect configuration was edited. No privileged Linux Hello
  service was enabled or started. A temporary, unprivileged session-bus daemon
  was started for the executable smoke test and shut down with SIGTERM.

## Results

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| Linux `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Windows Clippy for all non-GUI workspace crates/targets | Passed |
| Linux `cargo test --workspace --locked` | 43 passed; physical camera and two isolated-bus tests ignored in this invocation |
| Isolated session-bus tests, explicitly including the two ignored IPC tests | 2 passed; physical camera test not selected |
| Windows portable camera/core/face/storage/IPC/PAM/CLI tests | 35 passed |
| Linux `cargo build --workspace --release --locked` | Passed, including CLI, daemon and Slint GUI |
| Release CLI `--version`, `security`, `cameras` | Passed; security flags false, no video devices |
| Release daemon + CLI `status --session` on disposable D-Bus | Passed; protocol v1, diagnostic-only, UID resolved through the bus; clean SIGTERM exit |
| `cargo deny --locked check licenses sources bans` | Passed; duplicate-version warnings remain informational |
| `cargo audit` | Zero known vulnerability entries; two unmaintained-dependency advisories |
| `cargo audit --deny warnings` | **Failed as intended:** RUSTSEC-2025-0141 and RUSTSEC-2026-0192; no suppressions |
| D-Bus and inactive Polkit XML parsing; packaging shell syntax | Passed |
| `systemd-analyze verify` using a scratch copy pointing to the built daemon | Passed; WSL mount executable/world-writable mode warnings on the scratch file |
| Fedora RPM build/install/removal, SELinux, system-service execution | Not performed |

Linux has **45 passing tests** across the normal and explicit isolated-bus runs.
Windows exercises an overlapping portable subset, not 35 additional independent
security properties. The physical-camera test remains intentionally unexecuted.

The service verifier used the real service directives with only `ExecStart`
redirected to the local release binary, without installing anything under
`/usr/libexec` or starting the unit. It does not validate the service sandbox at
runtime, Fedora system-bus policy, SELinux policy, or RPM scriptlets.

## GUI verification

The compiled Slint interface was instantiated and rendered using Slint's headless
software window. All five tabs rendered at 980×740 and the minimum 760×660 size;
the resulting images were visually inspected. The [overview screenshot](screenshots/overview.png)
is from that executable renderer test, not a design mockup. It shows the actual
camera-off state, with no invented device or authentication status.

This confirms rendering and layout initialization, not interactive operation on
GNOME, KDE Plasma or Hyprland. Live camera preview, desktop theme propagation,
keyboard/screen-reader behavior and compositor-specific behavior still need
physical Fedora testing. Test-only screenshot export writes UI images, never
camera frames.

## What these checks establish

Tests cover bounded and malformed frames; format conversion; asynchronous queue
limits; cancellation, drop and reconnect identity; caller/context checks; event
ordering, challenge binding and expiry; unavailable PAD; numeric/quality guards;
synthetic enrollment/matching; authenticated encryption; tampering and wrong keys;
owner binding; Linux symlink/hard-link rejection; atomic save/load/delete; CLI
failure exit codes; real D-Bus transport and caller UID; concurrent diagnostic
requests; and a test-only fprintd protocol peer.

No synthetic biometric response is used to authenticate a real user. No raw
biometric sample or key was written into diagnostic reports. At the baseline recorded above, inference and PAD were explicit error/disabled
states. Optional inference was added in the native run below; PAD remains unavailable.

## Remaining gates

1. Run the documented hardware qualification on the actual Lenovo and record
   device/driver/format/emitter behavior, unplug/replug and cancellation results.
2. Complete detector/recognizer licensing and training-data provenance review,
   validate the new worker isolation more broadly, and evaluate accuracy and IR suitability.
   The optional pinned inference/alignment adapter is functional, not qualified.
3. Implement and independently evaluate PAD and the trusted acquisition path.
4. Review key provisioning, storage and revocation; evaluate TPM protection.
5. Implement trusted enrollment, privileged IPC authorization and the narrow PAM
   adapter; validate password fallback and recovery only in a disposable Fedora VM.
6. Resolve the two maintenance advisories and complete dependency/source notices.
7. Validate Fedora RPM lifecycle, SELinux, service hardening and supported desktops.

No accuracy, FAR/FRR, liveness, secure-erasure, hardware-backed trust or public
stable-release claim follows from these automated checks.

## Optional CPU inference on native Fedora (2026-10-11)

OpenCV 4.13.0 was installed; optional Rust binding `opencv` 0.101.1 and exact
YuNet/SFace hashes were added. Weights were fetched explicitly into ignored
`work/inference/models`, with upstream license/readme/provenance records. Neither
weights nor public fixture images are part of the source distribution.

| Check | Recorded result |
|---|---|
| Default workspace tests, locked graph | 49 passed, 4 ignored |
| Workspace tests with `hello-cli/face-opencv`, locked graph | 52 passed, 5 ignored; physical/model tests remain explicit |
| Workspace all-target clippy with optional adapter, `-D warnings` | Passed |
| Isolated session-bus integration | 2 passed |
| Python qualification/download boundaries | 8 passed, no network or camera |
| Explicit real-model public-fixture test | Passed: one face, repeatable 128 dimensions, blank/two-face rejection, invalid aligned inputs rejected |
| Live `/dev/video0` RGB face probe | Passed at 640×480; one functional 128-dimensional result, 109 ms for quality/detection/alignment/inference in this debug run |
| Optional release CLI compilation and smoke | Passed; security remains disabled and invalid models fail before camera access |
| Release live `/dev/video0` probe | Passed: 128 dimensions, 64 ms in this one release measurement |
| Live `/dev/video2` monochrome probe | Correctly rejected by diagnostic quality policy at 640×360, exit 1; no claim that gray/IR recognition works |
| Expanded-lockfile strict audit | Failed: same two maintenance advisories; 609 dependencies, zero known vulnerability entries |
| License/source/bans policy | Passed with warnings, no suppressions |

Live frames and embeddings were not written to disk or serialized into reports.
The retained camera reports are metadata-only JSON in `work/inference`. The public
OpenCV sample was used only for functionality tests; it is not an accuracy dataset.
Timing is a single measurement and excludes startup/model loading and acquisition.
This pre-isolation baseline used an in-process worker without a hard inference
deadline. It is superseded by the separate supervised-worker stage below; OpenCV
allocation wiping remains unguaranteed. Readiness flags and authentication stay
false, and no system service, enrollment, PAM or authselect configuration was enabled.
The Fedora CI job is added but has not been run on GitHub from this workstation.

## Requested stages 1 → 3 → 2 (2026-10-11)

### 1. Inference process isolation

Implemented an optional separate `hello-face-worker`, with private bounded pipes,
Linux syscall allowlist before native model parsing, no-new-privileges, disabled
core dumps/dumpability, 1 GiB address-space and five-second CPU limits. The parent
uses an eight-second wall deadline per initialization/request phase, kills the
private process group, and reaps on failure/cancellation. Model verification and
readiness precede camera access. No camera descriptor is passed to the worker.

| Check | Result |
|---|---|
| Worker timeout, blocked pipe, crash, malformed/model-incompatible/oversized output, cancellation | Passed; child reaped |
| Actual CPU exhaustion | Busy child terminated by the five-second CPU limit |
| Actual resource/syscall tests in a separate child | File/network/process creation denied; 2 GiB reservation rejected by 1 GiB address-space limit |
| Real models and public input in restricted worker | Passed, one face and 128 dimensions; no embedding in metadata |
| Full optional workspace tests | 63 passed, 7 ignored (explicit hardware/model tests and child-only sandbox test) |
| Default workspace tests | 49 passed, 4 ignored |
| All-target optional workspace clippy and formatting | Passed |
| Python recorder/downloader/header boundaries | 10 passed |
| Optional release CLI and worker | Compiled; benchmark passed |

The isolated live RGB probe in this session found no face and correctly rejected
it; this is not overwritten with the earlier user's successful image. The public
fixture supplies the positive functional inference check. Models and embeddings
still never enter the daemon/PAM authorization path. Linux x86_64 was tested;
aarch64 and other native OpenCV environments remain untested. This boundary does
not protect against compromised same-UID/root/kernel peers or guarantee wiping
OpenCV/kernel pipe/swap storage.

### 3. Monochrome/IR diagnosis

Actual `/dev/video2` GREY 640×360, Chicony `04f2:b7f5`: 120-frame series captured,
112 passed quality after the initial eight dark/settling frames. The CLI now chooses
a quality-acceptable candidate after warmup within a 30-frame bound. The settled
probe passed image quality but had zero detected faces, and returned failure.
Read-only standard controls showed Privacy=0, no standard exposure/emitter switch.
USB descriptors were inventoried; no extension-unit or emitter writes were made.
Lighting/face position, wavelength and emitter state were not independently
confirmed. Full IR recognition/illumination qualification is still open.
[Details and measured statistics](ir-qualification.md).

Follow-up extension-unit inventory: `inspect-uvc-controls.py` parsed the selected
USB interface and issued GET_LEN/GET_INFO only. IR exposed nine declared selectors
(eight advertise writes, one read-only); RGB exposed ten. Both inventories were
complete, as the normal user, without capture or control writes. The upstream
emitter tool's discovery/configuration code was reviewed at a pinned commit and
not executed. A matching GUID/selector/size from a different Lenovo model is a
research lead only; this exact firmware's emitter command remains unverified.
The Python suite now passes 15 tests, including five UVC boundary tests.

### Dataset-check follow-up

Added optional `face-dataset-check --manifest PATH`. It checks every declared image
before any inference and reports aggregate split counts and content-bound SHA-256
identifiers. Each identity must have two acquisition sessions. Evaluation rechecks
each sample digest before processing and aborts on replacements. New tests cover
late duplicate rejection before worker startup, changed files/symlinks, missing
sessions, short files, cancellation and label-free aggregate reports.

The optional CLI suite passed 19 tests (three explicitly ignored), the default CLI
suite passed three (one hardware test ignored), optional CLI clippy with warnings
denied passed, and release compilation/format/diff checks passed. The actual release
command accepted a generated eight-file constant-pixel structural fixture and
rejected the example's missing images with exit 1 and empty stdout. These synthetic
files contain no people and supply no face accuracy evidence. Reports are under
`work/inference/dataset-check-*.json`. No user camera pixels or embeddings were saved;
biometric accuracy, emitter mapping and authentication readiness remain open.

### Scoped IR face mode — emitter follow-up

The exact device accepted Microsoft XU unit 14/selector 6 face-mode payload
`01 03 02 00 00 00 00 00 00` (original `01 03 01 00 00 00 00 00 00`).
Native `IrEmitterLease` validates USB ID/revision/interface, GUID/selector bitmap,
capability layout and readback; it restores on session completion or ordinary
failure. Explicit CLI `--ir-emitter` and GUI **Use IR illumination** use this path.
SIGINT/SIGTERM handling now retains both receivers for cleanup.

Five 90-frame normal/IR/normal/IR/normal runs showed periodic bright/dark output
only with face mode enabled. All normal completions, both signals, cooperative
exclusion and RGB rejection restored the exact original bytes. The explicit Rust
hardware capture/stop/error-Drop test passed. These validate the activation path;
direct optical/spectral confirmation, physical conditions, recognition, suspend
and full authentication/PAD qualification remain separate outstanding checks.
No Realtek intensity writes, system emitter service, PAM install or extra runtime
dependency was introduced. No user pixels or embeddings were saved.

Final optional workspace: 69 normal tests passed, eight explicitly ignored.
The hardware test was run separately and passed. Workspace clippy with warnings
denied, release CLI/GUI compilation, 15 Python tests, fmt and diff checks passed.
The GUI checkbox was compiled and the existing software rendering test passed;
interactive GUI illumination use has not been independently exercised.
Source and measured evidence are detailed in [IR qualification](ir-qualification.md).

The operator subsequently confirmed illuminator and camera operation after the GUI
check instructions. This closes the observed activation issue on this laptop.
The report does not separately confirm blink timing or light-off after Stop;
register restoration is covered by the earlier hardware tests. Historical JSON
reports retain their original flags. Spectrum measurements, face accuracy and
authentication readiness are not established by this confirmation.

### 2. Model review and evaluation tools

Verified exact artifact headers: default-domain ONNX opset 11, float32 NCHW,
SFace 112×112 input / 128-dimensional output, no custom operator domains inventoried.
Reviewed pinned notices, YuNet's WIDER Face training source and SFace provenance.
Upstream issue 313 was checked through the GitHub API: open, four clarification
requests, no authoritative reply there as of this run. No redistribution approval
or verified exact SFace training-dataset mapping was inferred.

Added `face-benchmark` and `face-evaluate`, strict local dataset manifests,
identity-disjoint calibration/holdout, cross-session genuine comparisons, duplicate
image rejection, acquisition-failure accounting and descriptive Wilson bounds.
Unit data tests computation only, never supplies project biometric accuracy.
Release benchmark on five identical public inputs: median native 64 ms, P95 71 ms,
median startup 147 ms, peak worker RSS 207960 KiB. This is functional performance,
not FAR/FRR or a latency population study. No consented multi-identity/session
dataset has been supplied, so **actual biometric error rates remain unmeasured**.
[Review, protocol, commands and limits](model-evaluation.md).

Logs/metadata for all three stages are under `work/inference/`, including
`three-stages-workspace-tests.log`, `three-stages-workspace-clippy.log`,
`three-stages-real-worker-test.log`, `ir-quality-summary.json`,
`model-metadata.json` and `isolated-benchmark-release.json`. No user frames or
vectors were written to disk. PAM/authselect/service enablement and production
readiness flags remain unchanged and disabled.


## Optional GUI face diagnostics — 2026-10-11

Added the `hello-settings/face-diagnostics` feature and Camera → Check face.
The GUI and CLI share the bounded face probe; GUI selection also checks the
previously enumerated device identity. The GUI displays one-face/quality results
without frame files, embeddings, enrollment or authentication. Models are local
and pinned; the isolated worker remains a sibling executable. Preview and check
cannot run concurrently. Stop cancels either job; window closure awaits active
job cleanup before runtime teardown (camera stop, scoped IR restoration, worker
reaping). Process crashes/SIGKILL and kernel/device hangs remain outside this
ordinary shutdown guarantee.

Verified locally: 73 workspace tests passed, 8 opt-in tests ignored;
workspace all-target Clippy with `hello-settings/face-diagnostics` passed;
GUI rendering with the new controls passed at 980×740 and 760×660, default GUI
build check passed, release CLI/worker/GUI compilation passed, formatting and
diff whitespace checks passed. The release security report still denies system
authentication. Logs: `work/inference/gui-face-*.log` and
`work/inference/gui-face-security.json`. No new user face/camera capture was
performed during this GUI implementation; positive live GUI detection awaits
explicit Check face use. CI's Fedora optional job now covers the GUI feature;
remote CI has not been run. Strict dependency audit still reports the two known
unmaintained packages, with no known vulnerabilities; no suppressions added.

Build and run:

```sh
cargo build --locked --release -p hello-cli -p hello-settings --features hello-settings/face-diagnostics
LINUX_HELLO_MODELS="$PWD/work/inference/models" target/release/hello-settings
```


## User-confirmed GUI detection — 2026-10-11

After trying the optional GUI, the user reported “one face detected”. This
confirms positive live detection in that manual attempt, beyond the earlier
camera/illuminator confirmation. The message does not independently establish
camera modality, identity accuracy, illumination spectrum or PAD. No user image
or embedding was saved as evidence.


## In-memory reference comparison — 2026-10-11

Implemented optional GUI Remember face / Replace reference / Forget reference,
using one fresh bounded capture and a private in-memory embedding. Check face
performs a fresh probe and displays cosine similarity only. Camera identity,
IR mode and model/dimension compatibility are enforced. Rejected capture does
not replace the previous reference. Forget/replacement/destruction release the
zeroizing embedding. CLI reports retain their metadata-only format.

Verification: 77 workspace tests passed, 8 explicit opt-in tests ignored;
all-target workspace Clippy passed; the GUI's 5 tests passed including rendering
all pages at 980×740 and 760×660 with reference controls visible. Default GUI
build check, release CLI/worker/GUI build, format and whitespace checks passed.
The release security report still has all readiness flags false. Evidence:
`work/inference/session-reference-tests.log`, `session-reference-clippy.log`,
`session-reference-gui-tests.log`, `session-reference-default-check.log`,
`session-reference-release.log`, `session-reference-security.json`.
No new user capture or reference was automatically collected during development;
live reference/score behavior awaits the user's explicit button presses.
This feature does not implement persistent multi-sample enrollment, liveness or
identity authentication, and does not change the dependency audit warnings.


## Multi-sample GUI reference and encrypted persistence — 2026-10-11

User confirmed that session reference comparison works. Added Add sample (3
separate explicit acquisitions), duplicate/quality/coherence rejection,
authenticated camera/IR template binding and optional GUI passphrase persistence.
Save requires a completed three-sample template, verifies an existing file before
replacement, and leaves it unchanged on a wrong passphrase. Unlock restores a
reference in a fresh session without camera access. Clear memory clears memory
and pending samples; Delete saved removes the ciphertext and clears memory.
The public salt remains. Settings scrolls at minimum window size. CLI diagnostic
reports still contain no embeddings and all authentication flags remain false.

Local verification: 84 optional-feature workspace tests passed, 8 explicit opt-in
tests ignored; all-target workspace Clippy passed; GUI's 6 tests passed, including
masked-field validation and actual rendering at 980×740 and 760×660. Inspected
camera/settings screenshots with no camera capture. Default GUI check and release
CLI/worker/GUI build passed. Storage tests cover reopening with a passphrase,
wrong passphrases, authenticated binding, private modes, advisory locking,
missing/malformed/symlink salt and symlink directory rejection. Integration tests
use synthetic vectors, not personal face data; they confirm three-sample counting,
duplicates/coherence/mode rejection, fresh-session load, preservation on wrong
password, comparison and deletion. No user reference was automatically collected
or persisted during implementation. Live save/restart/unlock awaits explicit use.

Dependency review after adding pinned Argon2 0.6.0: audit reports zero known
vulnerabilities and the same two unmaintained warnings (`bincode`, `ttf-parser`);
strict audit remains failed. cargo-deny licenses/sources/bans passed. No warning
suppression. Format and whitespace checks passed. Logs/artifacts are ignored in
`work/inference/persistent-reference-*`. Remote CI was not run. Readiness, PAD,
trusted enrollment/IPC, evaluated thresholds and TPM/PAM remain unfinished.

Default-feature workspace verification also passed: 60 tests, 5 ignored; log `work/inference/persistent-reference-default-tests.log`.


## Detailed eight-sample acquisition — 2026-10-11

User confirmed that three-sample encrypted save/unlock works, then requested
more samples and more precise sampling. New references require eight accepted
acquisitions, with successive front/left/right/up/down/nearer/farther/front
instructions. These are guidance, not automatically verified pose coverage.
The isolated worker now calculates central-face exposure, contrast, edge-energy
and clipped-pixel fraction, plus face-box size/coverage/clearance. Add sample
requires these stricter measurements and ≥0.95 detection confidence before
advancing. Detailed backgrounds and box borders cannot substitute for usable
central-face detail. Errors give positioning/lighting/stillness suggestions and
preserve progress. Acquisition thresholds remain provisional, not measured FAR/FRR.

Old encrypted three-sample templates remain readable and show their actual sample
count on unlock. A new unfinished eight-sample set cannot overwrite the saved
reference. No automatically collected/saved user image or reference; all eight
samples still require explicit user clicks. Matching remains a cosine score;
all release authentication/readiness flags remain false.

Verification: 88 optional-feature workspace tests passed (8 ignored), 62 default
workspace tests passed (5 ignored), all-target workspace Clippy passed, release
CLI/worker/GUI build passed, 6 GUI tests including all-page rendering at 980×740
and 760×660 passed, minimal-size camera screenshot inspected. One explicit public
fixture test passed through real pinned models and seccomp worker, confirming
valid face-region metrics and no biometric vector in metadata. Added regressions
for small/clipped/soft/uncertain/nonfinite samples, detailed background/box-border
masking, unchanged sampling progress on rejection, eight-sample completion and
compatibility with old encrypted three-sample references. Format/diff checks pass.
Logs and screenshots: `work/inference/detailed-sampling-*`. Remote CI and live
user eight-sample acquisition were not run during implementation.


## Repeated Add sample rejection correction — 2026-10-11

User reported repeated “Sample rejected”; requested the full suffix to identify
their exact failed condition, which has not yet been supplied. Independently
reproduced the excessive detector cutoff on the public fixture: confidence
0.9089786, 145×206 face, central mean 0.529997/contrast 0.170589/edge 0.024045,
zero clipped pixels. It passed diagnostic inference but failed the new 0.95
sample gate. Gates now share 0.90; regression requires the public fixture to
pass the complete face-quality gate. No other quality thresholds were lowered.
Added maximum-three fresh capture retry for quality failures, stop on first
suitable frame, and immediate error/cancel propagation. GUI Diagnostics now
shows the last metadata report and exact result without biometric vectors.
The user subsequently confirmed that sampling works. Their original exact
rejection suffix was not supplied, so its particular failed gate remains unknown.

Verification: 90 workspace tests passed (8 ignored), Clippy passed, explicit pinned-model worker test passed, release CLI/worker/GUI build passed, fmt/diff checks passed. Release security remains disabled. Logs: work/inference/sampling-rejection-fix-*.


## Three-capture comparison diagnostic — 2026-10-11

User confirmed sampling works after the previous rejection correction. Added
Camera **Compare 3 samples** for a remembered/unlocked reference, reporting
aggregate median/minimum/maximum signed cosine scores after three fresh
quality-accepted probes. Actual reference and camera/IR binding are checked
before acquisition, including pending-only enrollment. Up to three quality
acquisitions per probe (nine total); duplicate frames, rejection, model/camera
mismatch, worker error or cancellation abort without partial summary. Uses the
existing tracked job, Stop, camera join, IR restoration and worker cleanup.
No reference update, automatic storage, identity threshold or login verdict.

Verification: 94 optional-feature workspace tests passed (8 ignored); all-target
workspace Clippy passed with warnings denied; default GUI cargo check passed;
release CLI/worker/GUI build passed; all-page GUI render at 980×740 and 760×660
passed and minimum-size Camera screenshot inspected; fmt/diff checks passed.
New regressions cover signed-score median/range, pending-only or wrong-mode
preflight without acquisition, quality/duplicate/model/capture failures preserving
reference, pre-cancellation and cancellation during acquisition. No live user
image captured for this change. Logs: work/inference/comparison-series-*.


## Experimental recognition of a saved face — 2026-10-11

Added Camera Recognize face for the completed eight-sample reference, requiring
matching camera/IR binding and valid explicit threshold before acquisition.
Typed ComparisonSummary is created only after three complete quality-checked
unique probes; all >= threshold => recognized, all < threshold => not recognized,
mixed => inconclusive. Acquisition errors/cancel produce no decision. Default
0.363 follows official OpenCV Zoo SFace example, checked online; it is not
calibrated for this sensor or centroid. Settings exposes experimental threshold
in (0, 1], not saved automatically. No system authorization or reference update.

Verification: 98 optional-feature workspace tests passed (8 ignored), all-target
workspace Clippy -D warnings passed, default GUI check passed, release
CLI/worker/GUI built. All-page render at 980×740 and 760×660 passed; Camera and
Settings at minimum size inspected (Settings scrolls). Added threshold-boundary,
nonfinite/invalid threshold, all-three consistency/mixed decision, full
eight-sample/binding and experimental UI message regressions. Existing
comparison rejection/cancellation and storage tests continue to pass.
Logs: work/inference/recognition-*. No live personal images were captured.
User needs to verify genuine and consented nonmatching faces on the actual
camera; no measured FAR/FRR or presentation attack resistance is claimed.


## Manual recognition trial counters — 2026-10-11

User confirmed experimental face recognition works. Added Settings manual
Test saved person / Test other person, Clear trials and Stop. Each explicit click
uses the existing three-probe recognition pipeline. Aggregate counters per
user-declared label separate recognition, nonrecognition, ambiguity and acquisition
failure; cancellation changes no counts/context. Counter updates and displayed
results commit together on the UI thread after worker/camera cleanup.

A trial set requires a completed eight-sample reference and is bound to a
process-unique reference generation and numeric threshold. Changes require Clear
trials before new capture, preventing mixed reference/threshold statistics.
Counters are bounded to 1000 outcomes, memory only, with no per-trial history or
images. Labels and conditions are not verified; no calibrated accuracy/FAR/FRR
is reported and no threshold is chosen automatically. Authentication stays off.

Verification: 101 optional-feature workspace tests passed (8 ignored), all-target
workspace Clippy -D warnings passed, default GUI check passed, release
CLI/worker/GUI built, all-page rendering at 980×740 and 760×660 passed. Minimum
Settings screenshot inspected; content scrolls. Added regressions for separate
label/outcome counts, cancellation not establishing context, threshold/reference
generation mixing, empty-reference rejection, reset and bounded trial capacity.
fmt/diff checks passed. Release security command retains disabled readiness.
No personal camera frames collected during implementation. Logs and snapshots:
work/inference/manual-trials-*. Live manually labelled trials remain for the user.


## Named person and guided UI — 2026-10-11

User requested a friendlier adding/recognition flow and person names. Added
Face tab (originally Polish) with name field, eight-step progress and pose guidance, explicit
next-sample/cancel controls, a separate recognition result card and rename.
Camera preview temporarily replaces the person cards; technical single-frame
checks/model controls moved under Ustawienia. New set preserves the previous
reference until completion and restores it on cancellation. Positive recognition
shows Recognized: [name]; nonmatching/ambiguous/error states do not name a match.

Template adds optional bounded display_name inside the existing authenticated
encrypted payload. No plaintext name sidecar/filename. Explicit Save persists
new names/renames. Old unnamed files remain readable; legacy sample counts and
eight-sample recognition requirements remain unchanged. Name is a user label,
not verified identity; all authentication flags stay false.

Verification: 104 optional-feature workspace tests passed (8 ignored), 63
default workspace tests passed (5 ignored); all-target workspace Clippy with
warnings denied passed; release CLI/worker/GUI built. Nine GUI tests including
all five pages and adding/ready/recognized/nonmatching states at 980×740 and
760×660 passed. Person UI snapshots inspected, progress fill anchored correctly
and primary controls made compact for the minimum window. Added name bounds,
Unicode/invalid name, legacy compatibility, encrypted name round-trip/no
plaintext, rejection/cancellation preserving previous person, rename preserving
similarity and named-result-only-on-positive-match regressions. Fixed default
GUI feature-gating of diagnostic-only tests. fmt/diff checks passed. Logs and
snapshots: work/inference/named-person-*. No live user acquisition performed.


## Scanner → name → recognition UI — 2026-10-11

User requested a visual scan, label and rescan/identify flow. Face now contains
a dark scanner panel, positioning oval, animated scan line, explicit 1/2/3 stages,
accepted-sample progress and named results. Panel scales at minimum window height.
One explicit scan starts/resumes eight bounded sample captures, with cancellable
1.5-second pose pauses; a rejection/error stops and retains accepted samples.
An anonymous completed template prompts for a name before GUI recognition.
Signing enables Scan and recognize for the existing three-probe comparison.

Optional parent FrameObserver displays actual acquired frames. One pending update
plus per-job alive/cancel checks bounds the UI queue and suppresses late frames.
Jobs clear the displayed image on completion/cancellation. Oval/animation are
guides/status, not detector bounds, measured pose, continuous live stream or PAD.
No image persisted or added to diagnostic reports; Slint buffers are not guaranteed
zeroized. Existing IR restoration, camera joins, isolated worker cleanup, named
encrypted template and disabled authentication gates remain intact.

Verification: 107 optional-feature workspace tests passed (8 ignored), 63 default
workspace tests passed (5 ignored), all-target Clippy -D warnings passed, release
CLI/worker/GUI built. Nine GUI tests passed including all pages and adding,
scanning, naming, ready, recognized/nonmatching states at 980×740 and 760×660.
Scanner/result/naming screenshots inspected. New regressions cover resumable
error, old-person preservation until completion, anonymous completion followed
by signing, cancellation not adding an in-flight sample and camera/IR/pre-cancel
preflight without acquisition or observer frames. Format/diff checks passed.
No live user images captured during implementation. Logs: work/inference/scanner-*.


## Strict owner comparison and motion checking — 2026-10-11

Implemented following a user-reported other-person false match. No raw image,
actual scores or reproducible owner/impostor dataset was supplied; the report is
not treated as calibrated accuracy evidence. GUI default/minimum threshold is
now 0.80 instead of the upstream demonstration value 0.363. Strict comparison
requires centroid and median exemplar similarity plus six normalized five-point
geometry ratios. Full exemplar and geometry data stay inside the explicitly
encrypted template; old templates require re-enrollment for this new path.

Recognition now measures five poses (front, random turn, front, opposite turn,
front), refuses unchanged or duplicate frames and binds template revision and
camera/IR through the attempt. Six bounded pose retries per stage, three quality
captures per retry, cancellable two-second pause. Every observed face passes the
identity gate; one low identity score rejects rather than being hidden by a high
median. Failed/cancelled/incomplete motion cannot produce a positive label.

Validation: 118 optional-feature workspace tests passed, 8 hardware/native tests
ignored; 66 default workspace tests passed, 5 ignored. Clippy with warnings denied
passed. Explicit real pinned-model/seccomp worker test on the existing public RGB
fixture passed and now checks geometry validity and its exclusion from metadata.
Regression tests cover a 0.70 embedding match that the old 0.363 gate accepted,
shape mismatch with identical embeddings, constant pose, repeated frames,
changed reference, camera/IR mismatch, cancellation, complete measured sequence,
strict encrypted save/unlock roundtrip, malformed geometry and old-template refusal.
The public fixture is not an owner/impostor or spoof-resistance evaluation.

This is experimental active motion checking, **not evaluated PAD**. Five landmarks
provide neither blinking, dense 3D shape nor depth. Print/display projective motion,
video replay, masks and injected streams remain unevaluated. pad_validated and
all authentication readiness flags stay false. No PAM/authselect/login changes.
A new physical enrollment and consented owner/impostor/spoof trials are still
needed on this camera. No biometric accuracy or spoof-resistance guarantee.


## IR settling and motion retry usability — 2026-10-11

User reported a saved owner failing at left/right turn or exposure gates. Camera
candidate selection previously accepted the first globally acceptable frame after
four startup frames; face-ROI exposure rejection then stopped the motion sequence.
The fix skips 18 IR / 8 RGB frames and prefers three stable acceptable exposures,
keeps ROI limits unchanged, retries transient acquisition/quality in the existing
six-attempt pose bound, separates dark/bright/clipped reasons, and shows measured
turn feedback including wrong direction. Required nose displacement is now 0.06
instead of 0.10. Default/minimum identity threshold remains 0.80, with exemplar,
shape, duplicate, cancellation and both-direction/return gates preserved.

122 optional-feature tests passed (8 ignored), 66 default tests passed (5 ignored),
Clippy with warnings denied passed. Four new regressions cover exposure startup/
instability reset, small vs static/wrong/extreme turns, bright/dark transient retry
without relaxing identity, and bounded persistent exposure failure with clear error.
No new physical owner-camera test was performed; user must rerun the release and
verify actual turn/exposure behavior. Existing strict encrypted templates remain
compatible and do not require a new scan for this fix. PAD remains unvalidated.


## Initial pose blocking and zero-percent feedback — 2026-10-11

User reported the first pose stuck at movement 0%. The initial baseline used a
fixed absolute normalized nose offset <=0.12; that is not calibrated to the owner
or IR landmark bias. Replaced with two consecutive distinct quality-accepted,
identity-matching observations within 0.05 of each other; their mean initializes
the per-attempt baseline. Quality/no-face rejection resets the pending pair.
No stable pair in six attempts aborts. Movement percentages now appear only during
turns; quality rejection has a distinct title and baseline asks the person to hold
still. All usable identity observations now enter final minimum-score policy,
including baseline samples not represented as separate completed motion stages.
No identity floor/shape/quality loosening, no skipped challenge, no saved-reference
migration or system-authentication changes.

126 optional-feature workspace tests passed (8 ignored), 66 default tests passed
(5 ignored), Clippy with warnings denied and release build passed. Regressions:
nonzero baseline 0.24/0.26 proceeds after a stable pair; alternating unstable
baseline fails bounded; lower initial identity evidence cannot pass a stricter
user-selected threshold; UI cannot label baseline or quality rejection as 0%
measured movement. Stationary frame/pose rejection tests still pass. Optional and
default executable integration tests were rerun sequentially to avoid shared debug
binary replacement between feature builds. No fresh physical owner-camera test;
user must close the old app and try the new release. Saved strict template remains
compatible. Experimental motion still does not establish evaluated PAD.


## Howdy source review and continuous person capture — 2026-10-11

See [the source analysis](howdy-analysis.md) for upstream commit, file-by-file
findings, measured acquisition evidence and limits. No Howdy installation or PAM
configuration change was performed. Jan confirmed an open shutter and lit emitter.

| Check | Result |
| --- | --- |
| Locked workspace tests, face-diagnostics feature | 129 passed, 10 ignored |
| Locked default workspace tests, run sequentially afterwards | 68 passed, 5 ignored |
| Explicit real-model worker tests | 2 passed, including face/blank/monochrome-CLAHE/face streaming in the restricted child |
| Actual continuous IR camera session | Passed: Two checks: 39 frames each, 6 / 7 dark frames skipped, 10 model inferences each (no faces); verified camera stop, emitter restoration and worker exit |
| All-target feature-enabled Clippy with warnings denied | Passed |
| Release CLI, sibling worker and GUI build | Passed |
| Formatting and diff whitespace check | Passed |
| Release security report | Privileged authentication, model/PAD/hardware/storage readiness remain false |

New regressions cover dark background versus useful signal, dark facial pixels
versus glare, dim sharp faces versus blurred input, and bounded stream packet,
time and cancellation behavior with child reaping. Actual GUI rendering remains
covered by the ordinary workspace tests. Physical logs retain aggregate metadata,
not camera pixels, landmarks, embeddings or identity labels. The physical test
with no detected face is acquisition/cleanup evidence only. Recognition of the
owner, held-out IR error rates and print/video attack resistance remain unverified.

Person capture now retains one bounded camera and loaded worker per operation;
previous per-attempt reopening and stable-global-exposure descriptions are
superseded by [the current acquisition design](inference-history.md#howdy-based-continuous-acquisition--2026-10-11).


## Patient scanning and stable instructions — 2026-10-11

New enrollment/recognition positions wait three seconds before sampling, and the
same position retries after 0.8 seconds. Recognition allows 16 attempts / 15
seconds per stage; enrollment allows eight attempts / 15 seconds per position.
Dark/uniform frames have a typed transient error; camera/model/protocol errors
remain fatal. Live searches use four inferences instead of ten, preserving the
192-frame and existing global time/CPU bounds. Pose instructions stay visible
when quality status changes. Identity and movement requirements are unchanged.

132 feature-enabled workspace tests and 68 default workspace tests passed.
The final GUI-specific run passed all 10 tests, including actual Slint rendering
and preserving the current pose in quality feedback. New regressions verify eight
bad frames before successful recognition, darkness recovery, bounded persistent
darkness, immediate protocol failure, and enrollment retry without discarding
accepted samples. Existing stationary-face, multiple-face, duplicate-frame,
low-identity, unstable-baseline and cancellation tests remain passing.
All-target feature-enabled Clippy, formatting, whitespace check and release
CLI/worker/GUI build passed. No new physical identity or PAD evaluation performed.


## First-stage baseline gap regression — 2026-10-11

Replaced the consecutive-good-frame baseline with a five-second pair window.
Both frames still require valid face quality, distinct digests, strict owner
similarity and yaw agreement <=0.05. Dark/no-face/quality gaps do not count as
owner frames. Expired/unstable candidates cannot complete the stage; a different
person after a gap still returns NotRecognized. No identity threshold changed.

135 optional workspace tests, 68 default workspace tests, all-target feature
Clippy, fmt/diff checks and release CLI/worker/GUI build passed. New regressions
cover valid owner -> poor quality -> dark -> no face -> valid owner -> all four
remaining poses, five-second expiration and unstable pair replacement, and an
impostor attempting to supply the second baseline frame. UI hints now distinguish
small face, crop/border problems and low detail. The user was asked for the exact
stage-one message; the change fixes a demonstrated software reset issue but does
not establish that it is the precise cause of the reported live stall. No new
physical owner-recognition or PAD validation was performed.


## Transitional frames during head turns — 2026-10-11

User confirmed camera signal and IR illumination while reporting turn refusal.
Recognition now permits two discarded weak identity/shape observations per turn,
then requires two stable, distinct, strictly matching owner observations to finish
that turn. A third weak observation fails even when strong frames alternate with
weak ones; good observations do not reset the failure budget. Front/return owner
checks remain immediate; the accepted floor 0.80 and all feature/shape gates are
unchanged. Summary metadata discloses the transition retry count. Enrollment
retries two inconsistent descriptors without adding them; its live hint requests
a smaller stationary turn. Persistent mismatch, duplicate frames and model errors
still fail. See the current temporal policy in inference.md.

138 optional workspace tests, 68 default workspace tests passed. New regressions
cover transient embedding and geometry failures followed by two stable turn
observations, alternating strong/weak frames exhausting the fixed budget, and
rejected enrollment descriptors omitted from the final eight accepted samples.
An additional targeted test verifies live enrollment retry hints. Existing first-
frame impostor, stationary face, changed reference, cancellation and worker tests
remain passing. Clippy with warnings denied, fmt/diff and release CLI/worker/GUI
build passed. These are software-policy checks, not a fresh physical owner scan,
IR accuracy calibration or PAD validation.


## Accuracy/refactor audit and measured functional performance — 2026-10-12

Full analysis and implementation decisions: [accuracy-improvements.md](accuracy-improvements.md).
No SCRFD/ArcFace checkpoint or ONNX Runtime dependency was introduced without
licensed NIR qualification. Working GUI, capture, encrypted storage, fingerprint,
daemon/systemd and authentication interfaces were preserved. PAM remains a stub
and production authentication was not modified.

145 optional workspace tests, 73 default workspace tests, final GUI10 tests and
claimed-user evaluation2 tests passed. Formatting, whitespace and all-target
Clippy -D warnings passed. Release CLI/worker/GUI built together. Explicit native
adapter1 and restricted worker2 tests passed, including exact canonical alignment,
RGB/BGR contract, grayscale/dim inputs, blank/multiple faces, state reset and
worker cleanup. Actual IR metadata-only sessions passed camera stop/emitter
restore. Before the acquisition-size fix, one face at confidence0.9027 was rejected
solely by width91<96 despite adequate ROI exposure/detail. The new limit uses
80×80 and inter-eye source sampling>=24px; the later IR check reported no face,
so physical owner verification after re-enrollment remains unvalidated.

New tests cover V2 pose selection without score cherry-picking, opposite-pose
arithmetic rejection in V1, frontal-anchor enforcement, model mismatch, corrupt
centroid/template migration, insufficient or mixed-owner sequences, claimed-user
and session leakage, and failed acquisitions in end-to-end FRR. These synthetic
results are software regressions, not measured population error rates.

20-repeat release functional benchmark on the existing public512×512 RGB fixture:
cold native mean68.25/P9571ms, loaded worker mean37.7/P9541ms; request mean112.05
versus77.7ms (startup excluded). Cold startup mean152.45ms ×20 versus147ms once.
Native CPU20frames1,366,666 versus754,751µs; peakRSS207,736 versus207,568KiB.
All20iterations per mode generated embeddings. The earlier release's cold native
median/P95 was67/71ms: no cold-model speedup or measured accuracy gain is claimed.
Logs work/inference/accuracy-20261012 are ignored, with no saved live pixels/vectors.

No representative consented IR dataset supplied: FAR/FRR and detection accuracy
were not measured. V2 face-evaluate now measures explicit claimed-user references
and multiframe attempts, calibrates on disjoint calibration identities, reports
held-out conditional/end-to-end FAR/FRR and FTA, and never installs its threshold
into PAM/GUI. The full GUI motion/PAD protocol remains a separate qualification.


## Passive front verification without mandatory turns — 2026-10-12

Ordinary recognition and labelled saved/other-person trials now collect three
fresh suitable owner-matching frames on a look-at-camera flow. Explicit protocol
classification distinguishes passive_front (3), measured_motion (5 + motion), and
comparison_only; passive verification never sets motion/PAD/authentication flags.
The previous motion experiment remains a separate diagnostic settings button.
Template/model/identity thresholds did not change. Stored full V1/V2 references
remain supported. Quality gaps may retry, but any suitable weak identity sample,
multiple faces, duplicate frame, changed reference, model error or cancellation
fails. Deadline is checked before/after acquisition; final progress rechecks Stop
and reference. UI result still follows camera/emitter/worker cleanup.

150 optional workspace tests, 73 default workspace tests, 10 GUI tests, all-target
Clippy, formatting/whitespace and release CLI/GUI/worker builds passed. New tests
cover three matching frames with no motion/PAD claim, first/third unknown frame,
partial two-frame rejection, eight quality failures before recovery, bounded
persistent no-face, duplicate samples, multiple faces, worker errors, reference
changes and cancellation including final progress. GUI rendering passed including
the separate diagnostic button. No fresh physical owner verification was performed.

## Opt-in sudo integration — 2026-10-12

Added optional `hello-pam/sudo-pam` and `hello-sudo/sudo-face`, root-owned
installation, UID-bound encrypted reference import, three-frame passive
verification, bounded child cancellation/reaping, sudo-only opt-in and rollback.
Default diagnostic daemon/PAM capabilities remain unqualified. Original sudo
password/account/session includes and authselect configuration are preserved.

On this Fedora host the module/helper/models were installed as root, an existing
IR reference was explicitly unlocked/imported through a masked owner terminal,
and live enrollment verification plus a second pre-activation match succeeded.
The installed management script activated only `/etc/pam.d/sudo`. A fresh
interactive `sudo -k; sudo id -u` returned0 without submitting a password. A
controlled root test temporarily removed the installed detector file, then ran
real sudo as UID1000 after clearing its timestamp: the original password prompt
appeared, correct password authentication returned0, and the model was restored
in `finally` and compared with the original. Another acquisition/nonmatch attempt
also fell back to password. `sudo -n` on this host refuses a fresh timestamp
before interactive authentication; do not use it as a face-verification test.

Before provisioning, actual Linux-PAM isolated-confdir tests passed: missing
reference/module never succeeded with a deny stack; permit fallback proceeded;
other service names did not gain a biometric success path. These temporary
permit fixtures were not installed into system PAM and executed no privileged
command. Rust tests cover timeout (including cancellation exit0), forced child
reaping, service/claimed-account restrictions, corrupt/cross-UID configuration,
invalid threshold and encrypted import checks. Existing passive matching and
storage regressions, default workspace, optional tests, Clippy, formatting,
18 Python tests and release compilation passed. Live owner matches are functional
checks, not representative FAR/FRR or presentation-attack measurements. PAD remains
unvalidated; no login/sshd/system-auth integration is enabled.

## KDE login integration preparation — 2026-10-12

This host actually runs plasma-login-manager6.7.5 (not SDDM); its PAM default is
`/usr/lib/pam.d/plasmalogin`. Implemented explicit `hello-pam/kde-pam`, per-user
root-only service opt-in, protected existing UID1000 reference reuse, root greeter
claim handling, exact one-substack PAM jump, vendor override backup/rollback and
live pre-activation probes in the actual SELinux `xdm_t` domain. sudo behavior is
preserved; screen lock/greeter/autologin/sshd are excluded. Added a tiny policy
allowing only xdm_t→v4l_device_t chr_file map (existing Fedora policy already allows
read/write/ioctl). No enforcing-mode change or blanket generated policy.

Installed compiled module/helper, root-only diagnostic probe, management/activation
scripts and SELinux policy. Root `xdm_t` negative PAM tests passed for absent/bad
marker, greeter/autologin/root account and permit/deny fallback dispatch. The
production camera and pinned models work in that domain. A captured live frame
scored0.5011 at provisional0.80 and correctly rejected; no owner-positive KDE test
is claimed from that attempt. A later strict pre-activation probe also rejected.
Both failures left production login PAM unchanged and removed the temporary marker.

Initial full-stack confdir probe failed with PAM_PERM_DENIED because include files
were absent in the test confdir; the harness now snapshots trusted vendor files
and admin/authselect overrides. Repeated actual KDE password fallback with the
installed detector deliberately unavailable succeeded with the original masked
password: PAM_AUTHENTICATE=0, PAM_ACCT_MGMT=0, PAM_SETCRED=0 in enforcing xdm_t.
Model and prior marker were restored in finally. No desktop session opened or
password written to a file/environment/argument. Added helper parent-death
SIGTERM supervision; root live subprocess test verified non-successful exit and
reaping within0.050s after parent exit (no acceptance after cancellation).

Rust default workspace, PAM5 + helper1 feature tests, Clippy default/feature,
formatting, release compile, C -Wall/-Wextra/-Werror, checkmodule/semodule_package,
shell syntax, diff checks and Python21 passed. PAM jump/reversible rollback tests
preserve preceding SELinux controls, later wallet hooks and account/session groups.
KDE activation still needs the enrolled owner facing the camera: activation
requires first an adapter-only match with deny fallback, then the proposed complete
PAM/account/setcred stack. An installed activate-kde.sh terminal was opened for
that owner test. No logout, desktop restart or physical greeter/session success
is claimed yet. Existing sudo remains enabled. See docs/kde-login.md for exact
activation, use, recovery and security limits. No PAD or representative NIR FAR/FRR
has been qualified.

## KDE PAM credential establishment fix — 2026-10-12

Owner-provided activation trace demonstrated a genuine PAM_AUTHENTICATE=0 and
PAM_ACCT_MGMT=0, followed by PAM_SETCRED=17 in the adapter-only probe. The previous
pam_sm_setcred unconditionally returned PAM_IGNORE; once the successful face
adapter was the selected auth module, that provided no successful credential
handler and Linux-PAM returned PAM_CRED_ERR.

The adapter now resets a private PAM-handle match marker before each authentication
attempt and records it only after the real verifier succeeds. A no-op setcred
returns success only for its own matched transaction; absent/reset/unrelated data
returns ignore, preserving password-module credential handling. No extra OS
credentials, password/AUTHTOK or biometric vectors are stored. Failure to reset
or record PAM module data fails to the original password stack. No threshold,
model, enrollment or PAM control flag was changed.

PAM6 feature tests, default PAM test, Clippy, formatting, release and Python21
passed. The updated module was installed root0644 and relabeled. Actual xdm_t
missing/corrupt opt-in, root/unsupported services and fallback tests still passed.
A mount-namespace-isolated regression test binds synthetic success/failure helper
executables and a dummy private state ONLY inside a new recursively private mount
namespace: successful simulated verifier -> actual PAM_AUTH/ACCT/SETCRED all0;
rejected simulated verifier -> AUTH7. No production helper, reference, PAM file or
OS session is changed; this measures dispatch, not biometric accuracy/owner match.
The probe must execute from the ordinary installed root filesystem: /run's nosuid
flag correctly rejects runcon domain transitions for a probe copied there.

An agent-triggered owner activation retry returned AUTH7 (no live match), so login
PAM remained inactive after that attempt. The owner's earlier genuine match is
from the supplied trace; a fresh complete real owner/greeter login is not yet
claimed. Existing sudo and password fallback remain in place. Re-run installed
activate-kde.sh while facing the enrolled camera to finish live activation.

### KDE activation diagnostics and readiness (2026-10-12)

Confirmed that the credential-fix module was installed: source release and
installed module SHA256 matched. A separate real xdm_t diagnostic captured one
IR face and rejected similarity0.712827 against0.80; camera/worker cleanup passed.
After installing this change, the actual strict activation PAM invocation reported
similarity0.653 against0.800, `centroid_similarity_below_threshold`, and AUTH7.
This distinguishes a real verifier rejection from the already corrected SETCRED17.
The subject's identity/presence was not independently established; these are not
labelled genuine/impostor accuracy measurements or FAR/FRR results. No threshold,
model, camera preprocessing or reference was modified to force acceptance.

Activation now separates sudo authorization from two labelled, three-second
countdown tests. Only the root-private test stacks request explicit diagnostics;
the installed production stack never receives that option. Diagnostics use the
same verifier, exit-status and timeout checks and disclose metadata/progress and
failure reasons, without embeddings/images. Cleanup/cancellation cannot be
reported as a match. Expected rejection/timeout returns a concise error and
removes the newly staged marker, with production login PAM unchanged.

Checks passed: PAM7 optional +1 default, helper1, passive verification5,
Python24, feature Clippy with warnings denied, formatting/release/shell syntax.
Real xdm_t negative marker/service/account and fallback-dispatch tests passed.
The private mount-namespace simulated-exit regression now covers both diagnostic
and ordinary mode: both complete AUTH/ACCT/SETCRED0 only for success; both reject
failure with AUTH7. These simulated tests are not biometric validation.
The updated helper/module/activation scripts were installed. KDE remains disabled
after the rejected real scan; sudo remains enabled and SELinux remains enforcing.

### Preserve a positive PAM result in the KDE parent stack (2026-10-12)

The owner's supplied trace now proves real three-frame matches in both scans
(minimum similarities0.853 and0.870). The adapter-only scan passed AUTH/ACCT/SETCRED;
the full parent returned AUTH6 despite matching. The previous success=1 jump
skipped the password substack without recording a positive authentication result.
Later optional wallet handlers did not supply one; Linux-PAM's dispatch retained
its initial PERM_DENIED result. This is a PAM-control bug, not an embedding error.

Replaced the numeric jump with a dedicated authentication substack containing
the sufficient face adapter followed by the unchanged password-auth substack.
Sufficient records success and terminates only the nested stack. Parent SELinux,
wallet/postlogin, account/password/session controls remain in place. Rejection
still invokes passwords; earlier required denial remains authoritative. The
manager preserves the exact original password line for rollback and migrates
the exact previous block. The companion is root-owned, has bounded fixed contents,
and is installed/relabelled before the service override only after both live tests.
Existing administrator changes to it are rejected rather than overwritten.

Python25 tests and diff/shell checks passed. A recursively private mount-namespace
regression with simulated helper exits covers the actual installed KDE parent
and included dependencies: success -> AUTH/ACCT/SETCRED0; rejection -> non-success
in ordinary and diagnostic modes; preceding required deny -> non-success. This
tests dispatch, not biometric accuracy. The old parent jump is explicitly checked
for AUTH6 when testing the unmodified vendor stack. A real xdm_t password fallback
test with the detector temporarily unavailable and original masked account
password completed AUTH/ACCT/SETCRED0; model/marker were restored in finally.
No desktop session was opened and no display-manager restart was performed.

The installed manager was updated. A new actual activation attempt rejected the
first scan at0.773/0.800 with centroid_similarity_below_threshold; identity/presence
was not independently established for that agent-triggered attempt. It did not
activate production KDE PAM. The owner must run the installed activation again
while facing the original camera. No threshold, model, enrollment, or SELinux
enforcement was weakened to force acceptance; existing sudo remains enabled.

### KScreenLocker self-account verification (2026-10-12)

The owner confirmed successful display-manager activation (two real minima0.868
and0.880, AUTH/ACCT/SETCRED0) and subsequently reported that actual login works.
Login is now enabled for jan; this observation does not validate PAD or NIR FAR/FRR.

Added separate `kde` locker opt-in, preserving the display-manager configuration.
Upstream6.7 and installed Fedora files confirm PAM runs unprivileged for this
service. The module accepts only PAM_USER matching its kernel real UID, and
invokes an installed small root-owned setuid launcher. The launcher accepts no
arguments, root/system callers, identities, commands, enrollment or diagnostics;
it validates protected own-account opt-in, fixed helper/config, disables dumps,
closes inherited descriptors, clears the environment, changes cwd to root and
execs only the existing fixed verifier with its real UID. Real UID remains the
caller so PAM timeout cancellation can signal its child. Biometric templates
remain root-private, and matching/security/cleanup rules are unchanged.

Rust PAM8 feature +1 default, feature Clippy, release, C warnings-as-errors,
Python26, formatting, shell and diff checks passed. The namespace-isolated real
PAM regression now covers the installed setuid launcher from a non-root caller:
simulated match completes AUTH/ACCT/SETCRED0; missing/corrupt locker marker and
mismatch reject. Extra launcher arguments/target IDs and root real UID reject.
Display-manager regression and preceding-required-denial tests still pass.
These fixture checks are dispatch tests, not biometric accuracy measurements.
A real unprivileged `kde` password fallback test with detector unavailable
completed AUTH/ACCT/SETCRED0 using masked account password. Detector/marker were
restored; neither a password nor raw biometric data was saved. No desktop was
locked/restarted during tests. SELinux remains enforcing without added policy.

Updated module, launcher(mode 4755), probe, manager and `activate-lock.sh` were
installed. A real activation attempt as the ordinary user returned AUTH7 in its
first strict scan; no production locker PAM change or new opt-in remained.
Owner identity/presence in this agent-triggered scan is not established. Existing
display-manager login remains enabled; locker activation still requires the
owner to run `activate-lock.sh` facing the enrolled camera for both real scans.

Subsequent owner confirmation (2026-10-12): unlocking after Win + L works.
This confirms the owner's successful use of the installed locker integration;
it is not a measured FAR/FRR or presentation attack evaluation. No additional
activation trace, latency measurements or camera samples were supplied.


## Unified setup and user-focused GUI — 2026-10-13

Implemented scan/name → three use cards → **Save and set up**. Ordinary mode
originally used Polish guidance; models, thresholds, motion tests and metadata are behind
the developer switch. Startup enumerates devices without capture, prefers an IR
candidate and uses installed pinned models when present. No root GUI or automatic
startup enrollment was introduced. A desktop menu entry was created on the host.

The GUI saves its encrypted reference, invokes the fixed installed helper through
pkexec, and sends bounded choices/source and a framed reference passphrase through
a private pipe. The administrator/OS account password is handled by the system
agent. Root setup derives its own subject UID, checks protected code before imports,
serializes changes and runs existing real import/verification and PAM checks.
Normal failure/result-pipe loss restores previous protected profile/PAM files.
A local save can remain successful while system setup fails; the UI distinguishes
these outcomes. Existing stricter root thresholds are preserved. Per-account sudo
disabling leaves other users' shared sudo path unchanged. Hard kill/power loss and
independent security review remain limitations.

Checks passed: feature PAM8, GUI11 (including exact response validation and actual
headless layout), helper2 (including bounded pipe and threshold preservation),
default PAM1, optional/default Clippy checks with warnings denied, formatting and
release build. Python31 tests passed, including request validation and rollback.
Documentation links passed. Rendered all ordinary pages at980×740 and760×660 and
inspected the settings screenshot; it contains UI only, no camera frames.

Installed updated root helper/PAM module and setup script. Real Linux-PAM dispatch
in a recursively private mount namespace verified per-account sudo disabling,
broken-marker rejection, other-account isolation, login/locker success/failure
and preceding-required-denial preservation. Synthetic verifier exits were used;
production references/PAM files were not changed by those fixtures. No new live
enrollment or end-to-end Polkit GUI setup with the owner's passphrase was performed.
The new workflow needs an owner-operated scan/save/system authorization for final
physical acceptance; existing previously confirmed sudo/login/unlock remains.
