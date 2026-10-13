//! Root-owned sudo verifier. Enrollment is an explicit, fresh sudo operation.
use hello_cli::diagnostics::{
    LiveCapture, RecognitionDecision, RecognitionThreshold, SessionReference,
    verify_person_observed,
};
use hello_pam::boundary::{self, protected_file, protected_path};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;
type Error = Box<dyn std::error::Error + Send + Sync>;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    uid: u32,
    device: PathBuf,
    identity: hello_camera::DeviceIdentity,
    threshold: f32,
    experimental_without_validated_pad: bool,
}
impl Config {
    fn validate(&self, uid: u32) -> Result<(), Error> {
        if self.version != 1
            || uid < 1000
            || self.uid != uid
            || !self.experimental_without_validated_pad
            || !self.threshold.is_finite()
            || !(0.80..=1.0).contains(&self.threshold)
            || !self.device.is_absolute()
            || !self.device.starts_with("/dev")
        {
            return Err("Invalid sudo enrollment configuration".into());
        }
        RecognitionThreshold::new(self.threshold)?;
        Ok(())
    }
}
fn directory(uid: u32) -> PathBuf {
    Path::new(boundary::ROOT).join(uid.to_string())
}
fn key(uid: u32) -> Result<hello_storage::SecretKey, Error> {
    let mut bytes = Zeroizing::new([0; 32]);
    let file = protected_file(&directory(uid).join("key"), true, 32)?;
    if file.metadata()?.len() != 32 {
        return Err("Invalid key length".into());
    }
    (&file).read_exact(&mut *bytes)?;
    Ok(hello_storage::SecretKey::from_bytes(*bytes))
}
fn load(uid: u32) -> Result<(Config, hello_face::Template), Error> {
    let config: Config = serde_json::from_reader(protected_file(
        &directory(uid).join("config.json"),
        true,
        16384,
    )?)?;
    config.validate(uid)?;
    let store = hello_storage::SecureStore::open(&directory(uid), uid, key(uid)?)?;
    let template = store.load()?.ok_or("No protected template")?;
    Ok((config, template))
}
async fn verify(uid: u32, cancel: Arc<AtomicBool>, diagnostic: bool) -> Result<(), Error> {
    let (config, template) = load(uid)?;
    let reference = SessionReference::from_template(template)?;
    reference.validate_strict_recognition(&config.identity, true)?;
    // Immutable installed sibling binary and exact pinned model files only.
    let executable = Path::new(boundary::HELPER).with_file_name("hello-face-worker");
    protected_file(&executable, false, 100_000_000)?;
    for name in [
        "face_detection_yunet_2023mar.onnx",
        "face_recognition_sface_2021dec.onnx",
    ] {
        protected_file(&Path::new(boundary::MODELS).join(name), false, 50_000_000)?;
    }
    let mut capture = LiveCapture::open(
        executable,
        config.device,
        PathBuf::from(boundary::MODELS),
        true,
        config.identity.clone(),
        cancel.clone(),
        None,
    )
    .await?;
    let capture = Arc::new(tokio::sync::Mutex::new(&mut capture));
    let result = verify_person_observed(
        &Mutex::new(reference),
        &config.identity,
        true,
        &cancel,
        || {
            let capture = capture.clone();
            let cancel = cancel.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                if cancel.load(Ordering::Acquire) {
                    return Err("Cancelled".into());
                }
                capture.lock().await.next_sample().await
            }
        },
        |progress| {
            if diagnostic {
                eprintln!("{}", serde_json::json!({"progress":progress.hint,"accepted_frames":progress.accepted,"required_frames":3}));
            }
        },
    )
    .await;
    let mut live = capture.lock().await;
    let cleanup = live.close().await;
    if diagnostic {
        eprintln!(
            "{}",
            serde_json::json!({"capture":live.report(),"threshold":config.threshold,"similarity":result.as_ref().ok().map(|r|r.minimum()),"failure_reason":result.as_ref().map(|r|r.failure_reason().map(str::to_owned)).unwrap_or_else(|error|Some(error.to_string())),"outcome":if cleanup.is_ok() && !cancel.load(Ordering::Acquire) && result.as_ref().is_ok_and(|r|r.classify(RecognitionThreshold::new(config.threshold).expect("validated threshold"))==RecognitionDecision::Recognized) {"match"}else{"rejected"},"cleanup_ok":cleanup.is_ok(),"pad_validated":false})
        );
    }
    cleanup?;
    let summary = result?;
    if cancel.load(Ordering::Acquire)
        || summary.classify(RecognitionThreshold::new(config.threshold)?)
            != RecognitionDecision::Recognized
    {
        return Err("Face did not match; use password authentication".into());
    }
    Ok(())
}
struct EchoGuard {
    fd: i32,
    original: libc::termios,
}
impl Drop for EchoGuard {
    fn drop(&mut self) {
        // SAFETY: tty stays open until this guard is dropped; termios was read from it.
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
    }
}
fn passphrase(cancel: &AtomicBool) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    let mut term = std::mem::MaybeUninit::uninit();
    // SAFETY: valid open tty descriptor and writable termios output.
    if unsafe { libc::tcgetattr(tty.as_raw_fd(), term.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful tcgetattr initialized the value.
    let original = unsafe { term.assume_init() };
    let mut hidden = original;
    hidden.c_lflag &= !libc::ECHO;
    // SAFETY: valid tty and initialized termios value.
    if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &hidden) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let guard = EchoGuard {
        fd: tty.as_raw_fd(),
        original,
    };
    tty.write_all(b"Haslo zapisanej osoby z GUI (nie haslo sudo): ")?;
    let mut password = Zeroizing::new(Vec::new());
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err("Enrollment cancelled".into());
        }
        let mut poll = libc::pollfd {
            fd: tty.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialized pollfd with a valid tty, bounded timeout.
        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(std::io::Error::last_os_error().into());
        }
        if ready == 0 {
            continue;
        }
        let mut byte = [0];
        tty.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            break;
        }
        if password.len() == 1024 {
            return Err("Passphrase too long".into());
        }
        password.push(byte[0]);
    }
    drop(guard);
    tty.write_all(b"\n")?;
    Ok(password)
}
fn enroll(name: &str, cancel: &AtomicBool) -> Result<u32, Error> {
    if boundary::account(name)?.0 < 1000 {
        return Err("System/root accounts cannot be enrolled".into());
    }
    let password = passphrase(cancel)?;
    import_reference(name, None, &password)
}
fn import_reference(name: &str, source: Option<&Path>, password: &[u8]) -> Result<u32, Error> {
    let (uid, _, home) = boundary::account(name)?;
    if uid < 1000 {
        return Err("System/root accounts cannot be enrolled".into());
    }
    let default_source = home.join(".local/share/linux-hello/diagnostic-reference");
    let source = source.unwrap_or(&default_source);
    eprintln!(
        "Importuje istniejacy wzorzec konta {name}. Pozniejsze zmiany w GUI nie zmienia wzorca sudo."
    );
    let template = hello_storage::SecureStore::import_passphrase(source, uid, password)?;
    if !template.strict_available() || template.dimensions() != 128 || template.sample_count() != 8
    {
        return Err("Incomplete or incompatible reference".into());
    }
    let matches = hello_camera::enumerate()?
        .into_iter()
        .filter(|d| {
            hello_cli::diagnostics::capture_binding(&d.identity, true).ok()
                == template.capture_binding()
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(
            "Original IR camera not found or ambiguous; use the camera used during enrollment"
                .into(),
        );
    }
    let device = &matches[0];
    let dir = directory(uid);
    protected_path(&dir)?;
    if !dir.exists() {
        fs::create_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    // Updating a face must never relax an administrator's existing threshold.
    let threshold = match protected_file(&dir.join("config.json"), true, 16384) {
        Ok(file) => previous_threshold(file, uid)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0.80,
        Err(error) => return Err(error.into()),
    };
    protected_path(&dir.join("key"))?;
    let keypath = dir.join("key");
    if !keypath.exists() {
        let mut bytes = Zeroizing::new([0; 32]);
        getrandom::fill(&mut *bytes)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&keypath)?;
        file.write_all(&*bytes)?;
        file.sync_all()?;
    }
    let store = hello_storage::SecureStore::open(&dir, uid, key(uid)?)?;
    store.save(&template)?;
    let config = Config {
        version: 1,
        uid,
        device: device.path.clone(),
        identity: device.identity.clone(),
        threshold,
        experimental_without_validated_pad: true,
    };
    config.validate(uid)?;
    let pending = dir.join(".config-pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&pending)?;
    file.write_all(&serde_json::to_vec_pretty(&config)?)?;
    file.sync_all()?;
    fs::rename(&pending, dir.join("config.json"))?;
    std::fs::File::open(&dir)?.sync_all()?;
    eprintln!("Wzorzec sudo zapisany. Patrz prosto w kamere — sprawdzam przed wlaczeniem PAM.");
    Ok(uid)
}
fn framed_password(mut input: impl Read) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut header = [0; 2];
    input.read_exact(&mut header)?;
    let length = usize::from(u16::from_be_bytes(header));
    if !(12..=1024).contains(&length) {
        return Err("Invalid passphrase length".into());
    }
    let mut password = Zeroizing::new(vec![0; length]);
    input.read_exact(&mut password)?;
    if std::str::from_utf8(&password)?.chars().count() < 12 {
        return Err("Passphrase requires twelve characters".into());
    }
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err("Unexpected passphrase input".into());
    }
    Ok(password)
}
fn previous_threshold(input: impl Read, uid: u32) -> Result<f32, Error> {
    let previous: Config = serde_json::from_reader(input)?;
    previous.validate(uid)?;
    Ok(previous.threshold)
}
#[tokio::main]
async fn main() {
    let result = run().await;
    if let Err(error) = result {
        eprintln!("Linux Hello: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Error> {
    // Snapshot the root PAM/sudo parent before any enrollment/inference work.
    // A cancelled graphical login must not leave its camera verifier orphaned.
    // SAFETY: getppid takes no pointers.
    let parent = unsafe { libc::getppid() };
    // SAFETY: libc calls use constants and an initialized resource-limit struct.
    if unsafe { libc::geteuid() } != 0 {
        return Err("Run the installed helper through sudo".into());
    }
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: limit is valid; prctl's integer arguments do not reference memory.
    let core_status = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) };
    // SAFETY: PR_SET_DUMPABLE takes only constant integer arguments.
    let dump_status = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if core_status != 0 || dump_status != 0 {
        return Err("Cannot protect biometric process".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let signals = cancel.clone();
    // Register before touching keys, cameras or prompting. Graceful cancellation
    // awaits native inference and verified emitter restoration before returning.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let watcher = tokio::spawn(async move {
        tokio::select! { _=terminate.recv()=>(), _=interrupt.recv()=>() };
        signals.store(true, Ordering::Release);
    });
    // SAFETY: PDEATHSIG takes a constant integer signal, no pointer argument.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) } != 0 {
        return Err("Cannot supervise biometric process parent".into());
    }
    // SAFETY: getppid has no preconditions. Close the registration race before
    // using keys/camera; SIGTERM already has the graceful cancellation handler.
    if parent == 1 || unsafe { libc::getppid() } != parent {
        return Err("Authentication parent disconnected".into());
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.as_slice() {
        [command, uid] if command == "verify" => verify(uid.parse()?, cancel, false).await,
        [command, name] if command == "enroll" => {
            let uid = enroll(name, &cancel)?;
            verify(uid, cancel, true).await
        }
        [command, name, source] if command == "enroll-stdin" => {
            // Root-authorized GUI setup only. Secret arrives on a bounded private
            // pipe, never argv/environment/file. Source import still enforces UID
            // ownership, private files, no symlinks and encrypted account binding.
            let password = framed_password(std::io::stdin().lock())?;
            let uid = import_reference(name, Some(Path::new(source)), &password)?;
            drop(password);
            verify(uid, cancel, false).await
        }
        [command, uid] if command == "diagnose" => verify(uid.parse()?, cancel, true).await,
        _ => Err("Usage: hello-sudo enroll USER | verify UID | diagnose UID".into()),
    };
    watcher.abort();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_import_pipe_rejects_oversize_truncation_and_extra_input() {
        let mut frame = 12_u16.to_be_bytes().to_vec();
        frame.extend_from_slice(b"test-phrase!");
        assert!(framed_password(frame.as_slice()).is_ok());
        frame.push(0);
        assert!(framed_password(frame.as_slice()).is_err());
        assert!(framed_password(&frame[..5]).is_err());
        assert!(framed_password(&1025_u16.to_be_bytes()[..]).is_err());
    }
    #[test]
    fn refuses_missing_corrupt_or_cross_user_configuration() {
        assert!(load(u32::MAX).is_err());
        let mut c = Config {
            version: 1,
            uid: 1000,
            device: "/dev/video2".into(),
            identity: Default::default(),
            threshold: 0.8,
            experimental_without_validated_pad: true,
        };
        assert!(c.validate(1000).is_ok());
        c.threshold = 0.93;
        let bytes = serde_json::to_vec(&c).unwrap();
        assert_eq!(previous_threshold(bytes.as_slice(), 1000).unwrap(), 0.93);
        assert!(previous_threshold(bytes.as_slice(), 1001).is_err());
        c.threshold = 0.80;
        assert!(c.validate(1001).is_err());
        for threshold in [f32::NAN, f32::INFINITY, 0.79, 1.01] {
            c.threshold = threshold;
            assert!(c.validate(1000).is_err());
        }
        assert!(serde_json::from_str::<Config>("{broken").is_err());
    }
}
