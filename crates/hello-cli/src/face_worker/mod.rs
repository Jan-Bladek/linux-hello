//! Bounded private-pipe supervision; biometric values never reach CLI reports.
mod limits;
use hello_face::{
    AlignedFace, Detection, Embedding, FaceEngine, FaceError, Frame, ModelSpec, PixelFormat,
    QualityMetrics, extract_embedding,
    opencv_engine::{OpenCvEngine, PinnedModels, RECOGNIZER_SHA256},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub const WALL_SECONDS: u64 = 8;
pub const MAX_WIDTH: u32 = 1920;
pub const MAX_HEIGHT: u32 = 1080;
const READY: &[u8; 8] = b"LHRDY004";
const MAX_REPLY: usize = 4096;
pub const MAX_STREAM_FRAMES: usize = 192;
const SESSION_SECONDS: u64 = 60;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    version: u8,
    pub embedding: Option<Embedding>,
    pub failure: Option<FaceError>,
    pub quality: QualityMetrics,
    pub faces: Option<usize>,
    pub confidence: Option<f32>,
    pub sample_quality: Option<hello_face::sampling::SampleQuality>,
    pub geometry: Option<hello_face::geometry::FaceGeometry>,
    pub inference_ms: u64,
    pub cpu_time_us: u64,
    pub peak_rss_kib: u64,
}

impl Diagnostic {
    fn validate(&self) -> Result<(), Error> {
        if self.geometry.as_ref().is_some_and(|g| !g.valid())
            || self.sample_quality.is_some_and(|q| !q.valid())
            || self.version != 1
            || self.embedding.is_some() == self.failure.is_some()
            || self.faces.is_some_and(|n| n > 500)
            || self
                .confidence
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            || [
                self.quality.mean_intensity,
                self.quality.contrast,
                self.quality.edge_energy,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("invalid inference worker reply".into());
        }
        if let Some(embedding) = &self.embedding
            && (embedding.dimensions() != 128
                || embedding.model_digest() != hello_face::pipeline_identity::digest()?
                || self.faces != Some(1)
                || self.geometry.is_none()
                || self.sample_quality.is_none_or(|quality| {
                    quality.rejection(self.confidence.unwrap_or(0.0)).is_some()
                })
                || !self.quality.has_signal()
                || !self
                    .confidence
                    .is_some_and(|v| v >= hello_face::sampling::MIN_DETECTION_CONFIDENCE))
        {
            return Err("inference worker returned incompatible biometric data".into());
        }
        Ok(())
    }

    /// Deliberately excludes biometric vectors and landmark coordinates.
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"diagnostic_passed": self.embedding.is_some(),
            "recognizer_sha256":RECOGNIZER_SHA256,"pipeline_identity":hello_face::pipeline_identity::current().ok(),"preprocessing":"opencv_sface_bgr8_to_rgb_nchw_raw_v1",
            "embedding_dimensions": self.embedding.as_ref().map(Embedding::dimensions),
            "rejection": self.failure.as_ref().map(ToString::to_string),
            "quality_failure_reason":self.sample_quality.and_then(|q| q.rejection(self.confidence.unwrap_or(0.0))),
            "landmark_geometry_available":self.geometry.is_some(),
            "quality": self.quality, "sample_quality": self.sample_quality, "faces": self.faces, "detection_confidence": self.confidence,
            "inference_ms": self.inference_ms, "cpu_time_us":self.cpu_time_us, "embedding_generated":self.embedding.is_some(), "worker_peak_rss_kib": self.peak_rss_kib,
            "raw_frames_saved": false, "embedding_saved": false, "authentication_ready": false})
    }
}

pub struct Worker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    finished: bool,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
    pub startup_ms: u128,
    spawned_at: Instant,
    streaming: bool,
    requests: usize,
}

impl Worker {
    /// Trusted explicit executable, never searched through PATH or an environment override.
    pub fn spawn(executable: &Path, models: &Path, cancel: Arc<AtomicBool>) -> Result<Self, Error> {
        let mut command = Command::new(executable);
        command
            .arg(models)
            .env_clear()
            .env("OPENBLAS_NUM_THREADS", "1")
            .env("OMP_NUM_THREADS", "1")
            .env("OPENCV_FOR_THREADS_NUM", "1");
        Self::spawn_command(command, cancel, Duration::from_secs(WALL_SECONDS))
    }

    pub fn spawn_stream(
        executable: &Path,
        models: &Path,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let mut command = Command::new(executable);
        command
            .arg(models)
            .arg("--stream")
            .env_clear()
            .env("OPENBLAS_NUM_THREADS", "1")
            .env("OMP_NUM_THREADS", "1")
            .env("OPENCV_FOR_THREADS_NUM", "1");
        Self::spawn_command_mode(command, cancel, Duration::from_secs(WALL_SECONDS), true)
    }
    fn spawn_command(
        command: Command,
        cancel: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Result<Self, Error> {
        Self::spawn_command_mode(command, cancel, timeout, false)
    }
    fn spawn_command_mode(
        mut command: Command,
        cancel: Arc<AtomicBool>,
        timeout: Duration,
        streaming: bool,
    ) -> Result<Self, Error> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if streaming {
            limits::configure_stream(&mut command);
        } else {
            limits::configure(&mut command);
        }
        let spawned_at = Instant::now();
        let mut child = command.spawn()?;
        let input = child.stdin.take().ok_or("worker stdin missing")?;
        let output = child.stdout.take().ok_or("worker stdout missing")?;
        let worker = Self {
            child,
            input,
            output,
            finished: false,
            cancel,
            timeout,
            startup_ms: 0,
            spawned_at,
            streaming,
            requests: 0,
        };
        limits::nonblocking(worker.input.as_raw_fd())?;
        limits::nonblocking(worker.output.as_raw_fd())?;
        Ok(worker)
    }

    pub fn initialize(mut self) -> Result<Self, Error> {
        let started = self.spawned_at;
        let mut ready = [0; 8];
        self.receive(&mut ready, started + self.timeout)?;
        if &ready != READY {
            return Err("invalid inference worker readiness reply".into());
        }
        self.startup_ms = started.elapsed().as_millis();
        Ok(self)
    }

    fn check(&self, deadline: Instant) -> Result<(), Error> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("inference worker cancelled and reaped".into());
        }
        if Instant::now() >= deadline
            || (self.streaming && self.spawned_at.elapsed() >= Duration::from_secs(SESSION_SECONDS))
        {
            return Err("inference worker exceeded wall deadline and was reaped".into());
        }
        Ok(())
    }

    fn receive(&mut self, mut bytes: &mut [u8], deadline: Instant) -> Result<(), Error> {
        while !bytes.is_empty() {
            self.check(deadline)?;
            match self.output.read(bytes) {
                Ok(0) => {
                    return Err(
                        "inference worker failed (ModelArtifact, limit, crash or incomplete reply)"
                            .into(),
                    );
                }
                Ok(n) => {
                    bytes = &mut bytes[n..];
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn send(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), Error> {
        while !bytes.is_empty() {
            self.check(deadline)?;
            match self.input.write(bytes) {
                Ok(0) => return Err("inference worker closed input".into()),
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn infer(mut self, width: u32, height: u32, rgb: &[u8]) -> Result<Diagnostic, Error> {
        let diagnostic = self.infer_reply(width, height, rgb)?;
        if self.streaming {
            self.send_stop()?;
        }
        self.wait_clean()?;
        Ok(diagnostic)
    }
    pub fn infer_frame(
        &mut self,
        width: u32,
        height: u32,
        rgb: &[u8],
    ) -> Result<Diagnostic, Error> {
        if !self.streaming {
            return Err("multi-frame inference requires a stream worker".into());
        }
        self.infer_reply(width, height, rgb)
    }
    fn infer_reply(&mut self, width: u32, height: u32, rgb: &[u8]) -> Result<Diagnostic, Error> {
        if self.requests >= MAX_STREAM_FRAMES {
            return Err("inference session frame budget exhausted".into());
        }
        if rgb.len() != frame_length(width, height)? {
            return Err("invalid inference frame byte count".into());
        }
        self.requests += 1;
        let deadline = Instant::now() + self.timeout;
        self.send(&width.to_le_bytes(), deadline)?;
        self.send(&height.to_le_bytes(), deadline)?;
        self.send(rgb, deadline)?;
        let mut header = [0; 4];
        self.receive(&mut header, deadline)?;
        let length = u32::from_le_bytes(header) as usize;
        if length == 0 || length > MAX_REPLY {
            return Err("oversized inference worker reply".into());
        }
        let mut reply = Zeroizing::new(vec![0; length]);
        self.receive(&mut reply, deadline)?;
        let diagnostic: Diagnostic = serde_json::from_slice(&reply)?;
        diagnostic.validate()?;
        Ok(diagnostic)
    }
    fn send_stop(&mut self) -> Result<(), Error> {
        let deadline = Instant::now() + self.timeout;
        self.send(&[0; 8], deadline)
    }
    pub fn finish(mut self) -> Result<(), Error> {
        if self.streaming {
            self.send_stop()?;
        }
        self.wait_clean()
    }
    fn wait_clean(&mut self) -> Result<(), Error> {
        let deadline = Instant::now() + self.timeout;
        loop {
            self.check(deadline)?;
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                if !status.success() {
                    return Err("inference worker crashed or exceeded resource limits".into());
                }
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let mut trailing = [0];
        if self.output.read(&mut trailing)? != 0 {
            return Err("trailing inference worker output".into());
        }
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.finished {
            limits::kill_group(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.finished = true;
        }
    }
}

fn frame_length(width: u32, height: u32) -> Result<usize, Error> {
    if !(16..=MAX_WIDTH).contains(&width) || !(16..=MAX_HEIGHT).contains(&height) {
        return Err("inference frame outside bounded shape".into());
    }
    Ok(width as usize * height as usize * 3)
}

struct TrackedEngine {
    engine: OpenCvEngine,
    faces: Option<usize>,
    confidence: Option<f32>,
    sample_quality: Option<hello_face::sampling::SampleQuality>,
    geometry: Option<hello_face::geometry::FaceGeometry>,
}
impl FaceEngine for TrackedEngine {
    fn model_spec(&self) -> Result<ModelSpec, FaceError> {
        self.engine.model_spec()
    }
    fn detect(&mut self, frame: &Frame<'_>) -> Result<Vec<Detection>, FaceError> {
        let detections = self.engine.detect(frame)?;
        self.faces = Some(detections.len());
        self.confidence = if detections.len() == 1 {
            Some(detections[0].confidence)
        } else {
            None
        };
        self.geometry = if let [detection] = detections.as_slice() {
            hello_face::geometry::FaceGeometry::measure(detection).ok()
        } else {
            None
        };
        self.sample_quality = if let [detection] = detections.as_slice() {
            hello_face::sampling::SampleQuality::measure(frame, detection).ok()
        } else {
            None
        };
        Ok(detections)
    }
    fn align(
        &mut self,
        frame: &Frame<'_>,
        detection: &Detection,
    ) -> Result<AlignedFace, FaceError> {
        if self.geometry.is_none()
            || self
                .sample_quality
                .is_none_or(|quality| quality.rejection(detection.confidence).is_some())
        {
            return Err(FaceError::PoorQuality);
        }
        self.engine.align(frame, detection)
    }
    fn embed(&mut self, aligned: &AlignedFace) -> Result<Embedding, FaceError> {
        self.engine.embed(aligned)
    }
}

pub fn run_worker() -> Result<(), Error> {
    let mut arguments = std::env::args_os().skip(1);
    let directory = arguments
        .next()
        .ok_or("explicit model directory required")?;
    let streaming = match arguments.next() {
        None => false,
        Some(flag) if flag == "--stream" => true,
        _ => return Err("unexpected worker arguments".into()),
    };
    if arguments.next().is_some() {
        return Err("unexpected worker arguments".into());
    }
    if streaming {
        limits::apply_stream_limits()?;
    } else {
        limits::apply_limits()?;
    }
    let models = PinnedModels::read(Path::new(&directory))?;
    limits::drop_privileges()?;
    limits::restrict_syscalls()?;
    let mut engine = TrackedEngine {
        engine: OpenCvEngine::from_pinned_models(models)?,
        faces: None,
        confidence: None,
        sample_quality: None,
        geometry: None,
    };
    let mut output = io::stdout().lock();
    output.write_all(READY)?;
    output.flush()?;
    let mut input = io::stdin().lock();
    for index in 0..=MAX_STREAM_FRAMES {
        let mut header = [0; 8];
        input.read_exact(&mut header)?;
        if streaming && header == [0; 8] {
            return Ok(());
        }
        if index == MAX_STREAM_FRAMES {
            return Err("worker frame budget exhausted".into());
        }
        let width = u32::from_le_bytes(header[..4].try_into()?);
        let height = u32::from_le_bytes(header[4..].try_into()?);
        let mut rgb = Zeroizing::new(vec![0; frame_length(width, height)?]);
        input.read_exact(&mut rgb)?;
        let frame = Frame {
            width: width as usize,
            height: height as usize,
            stride: width as usize * 3,
            format: PixelFormat::Rgb8,
            pixels: &rgb,
        };
        let quality = QualityMetrics::measure(&frame)?;
        let started = Instant::now();
        let cpu_started = limits::cpu_time_us();
        engine.faces = None;
        engine.confidence = None;
        engine.sample_quality = None;
        engine.geometry = None;
        let result = extract_embedding(
            &mut engine,
            &frame,
            hello_face::sampling::MIN_DETECTION_CONFIDENCE,
        );
        let (embedding, failure) = match result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let result = Diagnostic {
            version: 1,
            embedding,
            failure,
            quality,
            faces: engine.faces,
            confidence: engine.confidence,
            sample_quality: engine.sample_quality,
            geometry: engine.geometry.take(),
            inference_ms: started.elapsed().as_millis() as u64,
            cpu_time_us: limits::cpu_time_us().saturating_sub(cpu_started),
            peak_rss_kib: limits::peak_rss_kib(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&result)?);
        if bytes.len() > MAX_REPLY {
            return Err("worker reply exceeds protocol limit".into());
        }
        output.write_all(&(bytes.len() as u32).to_le_bytes())?;
        output.write_all(&bytes)?;
        output.flush()?;
        if !streaming {
            return Ok(());
        }
    }
    Err("worker frame budget exhausted".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stub(script: &str, timeout: Duration) -> Worker {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]).env_clear();
        Worker::spawn_command(command, Arc::new(AtomicBool::new(false)), timeout).unwrap()
    }
    #[test]
    fn syscall_and_resource_boundary_is_enforced_in_a_child() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "face_worker::tests::sandbox_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .env("OPENBLAS_NUM_THREADS", "1")
            .env("OMP_NUM_THREADS", "1");
        limits::configure(&mut command);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    #[ignore = "runs only in its separately restricted child process"]
    fn sandbox_child() {
        limits::apply_limits().unwrap();
        limits::restrict_syscalls().unwrap();
        assert_eq!(
            std::fs::File::open("/etc/passwd")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EPERM)
        );
        assert_eq!(
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EPERM)
        );
        assert!(Command::new("/bin/true").status().is_err());
        // A virtual reservation larger than RLIMIT_AS must fail, even before RAM use.
        assert!(
            Vec::<u8>::new()
                .try_reserve_exact(2 * 1024 * 1024 * 1024)
                .is_err()
        );
    }

    #[test]
    fn cpu_limit_terminates_a_busy_child() {
        let worker = stub("while :; do :; done", Duration::from_secs(8));
        let pid = worker.child.id();
        let started = Instant::now();
        assert!(worker.initialize().is_err());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn stalled_and_crashed_workers_are_reaped() {
        let started = Instant::now();
        let worker = stub("sleep 60", Duration::from_millis(100));
        let pid = worker.child.id();
        assert!(worker.initialize().is_err());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(stub("exit 7", Duration::from_secs(1)).initialize().is_err());
    }
    #[test]
    fn recognizer_only_worker_protocol_is_rejected_before_capture() {
        let worker = stub("printf LHRDY003; sleep 60", Duration::from_secs(1));
        let pid = worker.child.id();
        assert!(worker.initialize().is_err());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn cancellation_reaps_without_waiting_for_native_completion() {
        let worker = stub("sleep 60", Duration::from_secs(8));
        let pid = worker.child.id();
        worker.cancel.store(true, Ordering::Release);
        assert!(worker.initialize().is_err());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn oversized_reply_and_blocked_input_fail_within_deadline() {
        let worker = stub(
            "printf LHRDY004; printf '\\377\\377\\377\\377'; sleep 60",
            Duration::from_millis(100),
        )
        .initialize()
        .unwrap();
        let pid = worker.child.id();
        assert!(worker.infer(16, 16, &[127; 16 * 16 * 3]).is_err());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        let worker = stub("printf LHRDY004; sleep 60", Duration::from_millis(100))
            .initialize()
            .unwrap();
        let pid = worker.child.id();
        assert!(
            worker
                .infer(
                    MAX_WIDTH,
                    MAX_HEIGHT,
                    &vec![127; MAX_WIDTH as usize * MAX_HEIGHT as usize * 3]
                )
                .is_err()
        );
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn stream_budget_deadline_and_cancellation_reap_the_child() {
        for condition in 0..3 {
            let mut worker = stub("printf LHRDY004; sleep 60", Duration::from_secs(1))
                .initialize()
                .unwrap();
            worker.streaming = true;
            let pid = worker.child.id();
            match condition {
                0 => worker.requests = MAX_STREAM_FRAMES,
                1 => worker.spawned_at = Instant::now() - Duration::from_secs(SESSION_SECONDS + 1),
                _ => worker.cancel.store(true, Ordering::Release),
            }
            let started = Instant::now();
            assert!(worker.infer_frame(16, 16, &[127; 16 * 16 * 3]).is_err());
            drop(worker);
            assert!(!Path::new(&format!("/proc/{pid}")).exists());
            assert!(started.elapsed() < Duration::from_secs(1));
        }
    }

    #[test]
    fn malformed_and_incompatible_replies_are_rejected() {
        let report = Diagnostic {
            version: 1,
            embedding: Some(Embedding::new([1; 32], vec![1.0; 128]).unwrap()),
            failure: None,
            quality: QualityMetrics {
                mean_intensity: 0.5,
                contrast: 0.1,
                edge_energy: 0.1,
            },
            faces: Some(1),
            confidence: Some(0.95),
            sample_quality: None,
            geometry: None,
            inference_ms: 1,
            cpu_time_us: 1,
            peak_rss_kib: 1,
        };
        assert!(report.validate().is_err());
        assert!(frame_length(u32::MAX, 1080).is_err());
    }
}
