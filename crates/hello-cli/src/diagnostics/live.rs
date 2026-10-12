//! One bounded camera/emitter/model session per explicit GUI operation.
use super::{FaceProbe, FrameObserver, cancelled};
use crate::face_worker::{Error, Worker};
use hello_camera::{CameraSession, CaptureConfig, DeviceIdentity, IrEmitterLease};
use hello_face::{FaceError, Frame, PixelFormat, QualityMetrics, sampling::LightHistogram};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use zeroize::Zeroizing;

type StatusObserver = Arc<dyn Fn(String) + Send + Sync>;
#[derive(Default, serde::Serialize)]
struct CaptureStats {
    inferences: u64,
    dark_frames: u64,
    no_face_frames: u64,
    face_quality_rejections: u64,
}

pub struct LiveCapture {
    frames: watch::Receiver<Option<Arc<hello_camera::Frame>>>,
    camera_job: Option<tokio::task::JoinHandle<Result<(), Error>>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    emitter: Option<Arc<IrEmitterLease>>,
    worker: Option<Worker>,
    started: Instant,
    startup_ms: u128,
    seen: Vec<[u8; 32]>,
    observed: Arc<AtomicU64>,
    stats: CaptureStats,
    status: Option<StatusObserver>,
    last_status: String,
    camera_metadata: serde_json::Value,
    last_frame: Option<serde_json::Value>,
    latencies: Vec<u64>,
    inference_cpu_us: u64,
}
impl LiveCapture {
    pub async fn open(
        executable: PathBuf,
        path: PathBuf,
        models: PathBuf,
        ir: bool,
        expected: DeviceIdentity,
        cancel: Arc<AtomicBool>,
        observer: Option<FrameObserver>,
    ) -> Result<Self, Error> {
        if cancel.load(Ordering::Acquire) {
            return Err("Skan anulowany".into());
        }
        // Validate/load pinned native models under seccomp BEFORE camera access.
        let worker = Worker::spawn_stream(&executable, &models, cancel.clone())?;
        let worker = tokio::task::spawn_blocking(move || worker.initialize()).await??;
        let startup_ms = worker.startup_ms;
        let device = tokio::task::spawn_blocking(hello_camera::enumerate)
            .await??
            .into_iter()
            .find(|d| d.path == path)
            .ok_or("Wybierz kamerę z listy urządzeń")?;
        if device.identity != expected {
            return Err(hello_camera::CameraError::DeviceChanged.into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("Skan anulowany".into());
        }
        let emitter = if ir {
            Some(Arc::new(IrEmitterLease::acquire(&device)?))
        } else {
            None
        };
        let mut config = CaptureConfig::preview(device)?;
        config.timeout = Duration::from_secs(55);
        config.max_frames = 3600;
        config.reconnect_attempts = 0;
        let camera_metadata = serde_json::json!({"device":path,"pixel_format":format!("{:?}",config.pixel_format),
            "width":config.width,"height":config.height,"ir_emitter_requested":ir,
            "nir_model_qualified":false});
        let mut session = CameraSession::start(config)?;
        let (tx, frames) = watch::channel(None);
        let stop = Arc::new(AtomicBool::new(false));
        let observed = Arc::new(AtomicU64::new(0));
        let task_stop = stop.clone();
        let task_cancel = cancel.clone();
        let task_emitter = emitter.clone();
        let task_observed = observed.clone();
        let camera_job = tokio::spawn(async move {
            let result: Result<(), Error> = async {
                let mut index = 0_u32;
                loop {
                    let frame = tokio::select! {biased;
                        _=cancelled(&task_stop)=>break,
                        _=cancelled(&task_cancel)=>break,
                        frame=session.next_frame()=>frame?,
                    };
                    let Some(frame) = frame else {
                        break;
                    };
                    if index == 0
                        && let Some(emitter) = &task_emitter
                    {
                        emitter.rearm_streaming()?;
                    }
                    task_observed.fetch_add(1, Ordering::Relaxed);
                    if let Some(observer) = &observer {
                        observer(&frame);
                    }
                    // Warm up once; do not require stable brightness across flashing IR frames.
                    if index >= if ir { 18 } else { 8 } && tx.send(Some(Arc::new(frame))).is_err() {
                        break;
                    }
                    index += 1;
                }
                Ok(())
            }
            .await;
            let stopped = session.stop().await;
            result?;
            stopped?;
            Ok(())
        });
        Ok(Self {
            frames,
            camera_job: Some(camera_job),
            stop,
            cancel,
            emitter,
            worker: Some(worker),
            started: Instant::now(),
            startup_ms,
            seen: Vec::new(),
            observed,
            stats: CaptureStats::default(),
            status: None,
            last_status: String::new(),
            camera_metadata,
            last_frame: None,
            latencies: Vec::new(),
            inference_cpu_us: 0,
        })
    }
    pub fn set_status_observer(&mut self, observer: StatusObserver) {
        self.status = Some(observer);
    }
    fn status(&mut self, message: String) {
        if self.last_status != message {
            self.last_status = message.clone();
            if let Some(observer) = &self.status {
                observer(message);
            }
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let mut latencies = self.latencies.clone();
        serde_json::json!({"mode":"continuous_camera_and_model_session","camera":self.camera_metadata,
        "last_analyzed_frame":self.last_frame,"native_inference":crate::evaluation::timing(&mut latencies),
        "inference_cpu_us":self.inference_cpu_us,
        "frames_observed":self.observed.load(Ordering::Relaxed),"analysis":self.stats,"model_startup_ms":self.startup_ms,
        "raw_frames_saved":false,"embedding_saved":false,"authentication_ready":false})
    }
    /// Search successive fresh frames within a time/frame budget, retaining no image history.
    pub async fn next_sample(&mut self) -> Result<FaceProbe, Error> {
        self.frames.borrow_and_update(); // Never analyze a queued frame from before the new pose.
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut last = None;
        let mut inference_attempts = 0;
        while Instant::now() < deadline && inference_attempts < 4 {
            if self.cancel.load(Ordering::Acquire) {
                return Err("Skan anulowany".into());
            }
            if self.started.elapsed() >= Duration::from_secs(50) {
                return Err("Upłynął czas sesji kamery. Uruchom skan ponownie.".into());
            }
            tokio::select! {biased;
                _=cancelled(&self.cancel)=>return Err("Skan anulowany".into()),
                _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>break,
                changed=self.frames.changed()=>changed.map_err(|_|"Strumień kamery zakończył się przed uzyskaniem próbki")?,
            }
            let Some(captured) = self.frames.borrow_and_update().clone() else {
                continue;
            };
            let rgb = Zeroizing::new(captured.to_rgb()?);
            let width = captured.width;
            let height = captured.height;
            let frame = Frame {
                width: width as usize,
                height: height as usize,
                stride: width as usize * 3,
                format: PixelFormat::Rgb8,
                pixels: &rgb,
            };
            let histogram = LightHistogram::measure(&frame)?;
            let quality = QualityMetrics::measure(&frame)?;
            if !histogram.has_signal() || !quality.has_signal() {
                self.stats.dark_frames += 1;
                self.status(
                    if histogram.white_fraction >= 0.995 {
                        "Kamera pokazuje biały obraz — odsuń mocne światło."
                    } else {
                        "Kamera pokazuje ciemny obraz — sprawdź zasłonę obiektywu i tryb IR."
                    }
                    .into(),
                );
                continue;
            }
            let mut hash = Sha256::new();
            hash.update(width.to_le_bytes());
            hash.update(height.to_le_bytes());
            hash.update(&*rgb);
            let digest: [u8; 32] = hash.finalize().into();
            if self.seen.contains(&digest) {
                return Err("Powtórzona klatka kamery — skan przerwany".into());
            }
            self.seen.push(digest);
            let mut worker = self
                .worker
                .take()
                .ok_or("Proces rozpoznawania nie jest dostępny")?;
            let (returned, result) = tokio::task::spawn_blocking(move || {
                let result = worker.infer_frame(width, height, &rgb);
                (worker, result)
            })
            .await?;
            self.worker = Some(returned);
            let mut result = result?;
            self.stats.inferences += 1;
            inference_attempts += 1;
            self.latencies.push(result.inference_ms);
            self.inference_cpu_us += result.cpu_time_us;
            self.last_frame = Some(result.metadata());
            let mut report = self.last_frame.as_ref().expect("frame metadata").clone();
            report["capture"] = self.report();
            report["frame_histogram"] = serde_json::to_value(histogram)?;
            report["sample_capture_attempts"] = inference_attempts.into();
            report["worker_isolated"] = true.into();
            report["worker_mode"] = "bounded_stream".into();
            report["worker_cpu_limit_seconds"] = 20.into();
            let probe = FaceProbe {
                report,
                embedding: result.embedding.take(),
                frame_digest: digest,
                quality: result.quality,
                faces: result.faces,
                sample_quality: result.sample_quality,
                confidence: result.confidence,
                geometry: result.geometry.take(),
            };
            if probe.faces.is_some_and(|n| n > 1) {
                return Ok(probe);
            }
            let usable = probe.embedding.is_some()
                && probe
                    .sample_quality
                    .is_some_and(|q| q.rejection(probe.confidence.unwrap_or(0.0)).is_none());
            if usable {
                self.last_status.clear();
                return Ok(probe);
            }
            if let Some(failure) = result.failure.as_ref()
                && !matches!(failure, FaceError::PoorQuality | FaceError::FaceCount)
            {
                return Err(format!("Model nie może przeanalizować obrazu: {failure}").into());
            }
            if probe.faces == Some(0) {
                self.stats.no_face_frames += 1;
                self.status("Nie widzę twarzy — patrz w kamerę, pokaż oba oczy.".into());
            } else {
                self.stats.face_quality_rejections += 1;
                let reason = probe
                    .sample_quality
                    .and_then(|q| q.rejection(probe.confidence.unwrap_or(0.0)))
                    .unwrap_or("face detail is too soft");
                self.status(super::quality_hint(reason));
            }
            last = Some(probe);
        }
        if let Some(probe) = last {
            return Ok(probe);
        }
        Err(super::TransientCapture("Czekam na doświetloną klatkę z kamery.").into())
    }
    /// Await all cleanup BEFORE publishing any recognition result.
    pub async fn close(&mut self) -> Result<(), Error> {
        self.stop.store(true, Ordering::Release);
        let camera_result = if let Some(job) = self.camera_job.take() {
            job.await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
        } else {
            Ok(())
        };
        // Release the latest private pixel buffer even if the caller retains this session.
        self.frames = watch::channel(None).1;
        let emitter_result = if let Some(emitter) = self.emitter.take() {
            match Arc::try_unwrap(emitter) {
                Ok(emitter) => emitter.restore().map_err(|e| e.to_string()),
                Err(_) => Err("Nie zwolniono kontroli emitera".into()),
            }
        } else {
            Ok(())
        };
        let worker_result = if let Some(worker) = self.worker.take() {
            tokio::task::spawn_blocking(move || worker.finish())
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
        } else {
            Ok(())
        };
        camera_result?;
        emitter_result?;
        worker_result?;
        Ok(())
    }
}
impl Drop for LiveCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
