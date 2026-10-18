//! Synthetic non-face tensors only. No camera, participant data or auth decision.
#![cfg(all(feature = "opencv", target_os = "linux"))]
use hello_face::{AlignedFace, FaceEngine, opencv_engine::OpenCvEngine};
use std::path::Path;
use zeroize::Zeroizing;

#[test]
#[ignore = "requires installed pinned public models; no camera or personal template"]
fn repeated_synthetic_inference_survives_native_buffer_erasure() {
    let mut engine =
        OpenCvEngine::load(Path::new("/usr/local/libexec/linux-hello-sudo/models")).unwrap();
    let input = |offset: usize| AlignedFace {
        width: 112,
        height: 112,
        channels: 3,
        values: Zeroizing::new(
            (0..112 * 112 * 3)
                .map(|i| ((i * 17 + offset) % 256) as f32)
                .collect(),
        ),
    };
    let a = input(0);
    let b = input(93);
    let first = engine.embed(&a).unwrap();
    let middle = engine.embed(&b).unwrap();
    let repeated = engine.embed(&a).unwrap();
    assert_eq!(first.dimensions(), 128);
    assert_eq!(middle.dimensions(), 128);
    assert!(first.similarity(&repeated).unwrap() > 0.99999);
    // This tests lifetime/buffer reuse, not recognition accuracy or liveness.
}
