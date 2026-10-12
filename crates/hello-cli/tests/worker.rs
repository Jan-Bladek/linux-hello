#![cfg(all(feature = "face-opencv", target_os = "linux"))]
use hello_cli::face_worker::Worker;
use std::{
    io::Read,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
use zeroize::Zeroizing;

#[test]
#[ignore = "requires explicit LINUX_HELLO_MODELS and LINUX_HELLO_TEST_RGB public raw fixture"]
fn real_models_run_in_restricted_worker_and_exit_cleanly() {
    let models = std::env::var_os("LINUX_HELLO_MODELS").unwrap();
    let fixture = std::env::var_os("LINUX_HELLO_TEST_RGB").unwrap();
    let mut rgb = Zeroizing::new(Vec::new());
    std::fs::File::open(fixture)
        .unwrap()
        .take(512 * 512 * 3 + 1)
        .read_to_end(&mut rgb)
        .unwrap();
    let worker = Worker::spawn(
        Path::new(env!("CARGO_BIN_EXE_hello-face-worker")),
        Path::new(&models),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .initialize()
    .unwrap();
    let result = worker.infer(512, 512, &rgb).unwrap();
    assert_eq!(result.embedding.as_ref().unwrap().dimensions(), 128);
    let sample_quality = result.sample_quality.unwrap();
    assert!(sample_quality.valid());
    assert!(
        sample_quality
            .rejection(result.confidence.unwrap())
            .is_none(),
        "known usable public face must not be excluded by a stricter detector threshold"
    );
    assert!(result.geometry.as_ref().unwrap().valid());
    let metadata = result.metadata();
    assert!(metadata.get("geometry").is_none());
    assert!(metadata.get("landmarks").is_none());
    assert_eq!(metadata["faces"], 1);
    assert_eq!(metadata["authentication_ready"], false);
    assert!(metadata.get("embedding").is_none());
    assert!(metadata["worker_peak_rss_kib"].as_u64().unwrap() < 1024 * 1024);
}

#[test]
#[ignore = "requires explicit public fixture and pinned models"]
fn real_stream_worker_loads_once_clears_stale_faces_and_finishes() {
    let models = std::env::var_os("LINUX_HELLO_MODELS").unwrap();
    let fixture = std::env::var_os("LINUX_HELLO_TEST_RGB").unwrap();
    let rgb = Zeroizing::new(std::fs::read(fixture).unwrap());
    let mut worker = Worker::spawn_stream(
        Path::new(env!("CARGO_BIN_EXE_hello-face-worker")),
        Path::new(&models),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .initialize()
    .unwrap();
    let first = worker.infer_frame(512, 512, &rgb).unwrap();
    assert_eq!(first.faces, Some(1));
    let blank = Zeroizing::new(vec![0; 512 * 512 * 3]);
    let empty = worker.infer_frame(512, 512, &blank).unwrap();
    assert!(empty.embedding.is_none());
    assert!(empty.geometry.is_none());
    assert!(empty.faces.is_none());
    assert!(empty.sample_quality.is_none());
    // A textured monochrome non-face exercises detector-only CLAHE under seccomp.
    let mut checker = Zeroizing::new(vec![0; 512 * 512 * 3]);
    for y in 0..512 {
        for x in 0..512 {
            let value = if (x / 8 + y / 8) % 2 == 0 { 40 } else { 100 };
            checker[(y * 512 + x) * 3..(y * 512 + x) * 3 + 3].fill(value);
        }
    }
    let no_face = worker.infer_frame(512, 512, &checker).unwrap();
    assert_eq!(no_face.faces, Some(0));
    assert!(no_face.embedding.is_none());
    assert!(no_face.geometry.is_none());
    let last = worker.infer_frame(512, 512, &rgb).unwrap();
    assert!(
        first
            .embedding
            .as_ref()
            .unwrap()
            .similarity(last.embedding.as_ref().unwrap())
            .unwrap()
            > 0.999
    );
    assert!(last.geometry.as_ref().unwrap().valid());
    worker.finish().unwrap();
}

#[tokio::test]
#[ignore = "explicit LINUX_HELLO_CAMERA/LINUX_HELLO_MODELS physical session, metadata only"]
async fn live_session_is_bounded_reports_quality_and_restores_camera() {
    let path = std::path::PathBuf::from(std::env::var_os("LINUX_HELLO_CAMERA").unwrap());
    let models = std::path::PathBuf::from(std::env::var_os("LINUX_HELLO_MODELS").unwrap());
    let device = hello_camera::enumerate()
        .unwrap()
        .into_iter()
        .find(|d| d.path == path)
        .unwrap();
    let ir = device
        .formats
        .iter()
        .any(|f| f.pixel_format == hello_camera::PixelFormat::Grey);
    let observed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let seen = observed.clone();
    let mut session = hello_cli::diagnostics::LiveCapture::open(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_hello-face-worker")),
        path,
        models,
        ir,
        device.identity,
        Arc::new(AtomicBool::new(false)),
        Some(Arc::new(move |_| {
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })),
    )
    .await
    .unwrap();
    let result = session.next_sample().await;
    let report = session.report();
    session.close().await.unwrap();
    assert!(observed.load(std::sync::atomic::Ordering::Relaxed) > 0);
    assert_eq!(report["raw_frames_saved"], false);
    assert_eq!(report["authentication_ready"], false);
    // Captures no identity and saves no pixels. No face in front of the sensor is
    // a supported outcome; diagnostics must distinguish this from a good sample.
    println!(
        "Physical capture metadata: {} / usable_face={}",
        report,
        result
            .as_ref()
            .is_ok_and(|probe| probe.report["diagnostic_passed"] == true)
    );
}
