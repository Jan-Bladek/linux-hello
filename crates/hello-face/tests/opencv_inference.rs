#![cfg(all(feature = "opencv", target_os = "linux"))]

use hello_face::{
    AlignedFace, FaceEngine, FaceError, Frame, PixelFormat, extract_embedding,
    opencv_engine::OpenCvEngine,
};
use std::{fs::File, io::Read, path::PathBuf};
use zeroize::Zeroizing;

/// Public OpenCV sample converted to raw 512x512 RGB; not a subject accuracy test.
/// Models and image must be supplied explicitly. No downloads during cargo test.
#[test]
#[ignore = "requires LINUX_HELLO_MODELS and LINUX_HELLO_TEST_RGB (512x512 raw RGB public fixture)"]
fn pinned_models_detect_align_embed_and_reject_invalid_input() {
    let directory =
        PathBuf::from(std::env::var_os("LINUX_HELLO_MODELS").expect("explicit models directory"));
    let path = PathBuf::from(
        std::env::var_os("LINUX_HELLO_TEST_RGB").expect("explicit public RGB fixture"),
    );
    let mut bytes = Zeroizing::new(Vec::new());
    File::open(path)
        .unwrap()
        .take(512 * 512 * 3 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes.len(), 512 * 512 * 3);
    let frame = Frame {
        width: 512,
        height: 512,
        stride: 512 * 3,
        format: PixelFormat::Rgb8,
        pixels: &bytes,
    };
    let mut engine = OpenCvEngine::load(&directory).unwrap();
    // Canonical landmarks must leave a 112x112 input unchanged (except RGB->BGR).
    // This checks the real OpenCV alignment contract, not a second implementation.
    let pattern = Zeroizing::new(
        (0..112 * 112 * 3)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let canonical = Frame {
        width: 112,
        height: 112,
        stride: 112 * 3,
        format: PixelFormat::Rgb8,
        pixels: &pattern,
    };
    let detection = hello_face::Detection {
        bounds: [0.0, 0.0, 112.0, 112.0],
        landmarks: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
        confidence: 1.0,
    };
    let aligned = engine.align(&canonical, &detection).unwrap();
    for (pixel, values) in pattern
        .as_chunks::<3>()
        .0
        .iter()
        .zip(aligned.values.as_chunks::<3>().0)
    {
        assert_eq!(
            *values,
            [
                f32::from(pixel[2]),
                f32::from(pixel[1]),
                f32::from(pixel[0])
            ]
        );
    }

    assert_eq!(engine.detect(&frame).unwrap().len(), 1);
    let first = extract_embedding(&mut engine, &frame, 0.9).unwrap();
    let second = extract_embedding(&mut engine, &frame, 0.9).unwrap();
    for multiplier in [0.8, 1.0] {
        let gray = Zeroizing::new(
            bytes
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| {
                    ((0.2126 * f32::from(p[0])
                        + 0.7152 * f32::from(p[1])
                        + 0.0722 * f32::from(p[2]))
                        * multiplier)
                        .round() as u8
                })
                .collect::<Vec<_>>(),
        );
        let monochrome = Frame {
            width: 512,
            height: 512,
            stride: 512,
            format: PixelFormat::Gray8,
            pixels: &gray,
        };
        let result = extract_embedding(&mut engine, &monochrome, 0.9).unwrap();
        assert_eq!(result.dimensions(), 128);
        assert!(result.similarity(&first).unwrap().is_finite());
    }

    assert_eq!(first.dimensions(), 128);
    assert_eq!(first.model_digest(), engine.model_digest().unwrap());
    assert!(first.similarity(&second).unwrap() > 0.99999);
    for value in [f32::NAN, -1.0, 256.0, 0.5] {
        let aligned = AlignedFace {
            width: 112,
            height: 112,
            channels: 3,
            values: Zeroizing::new(vec![value; 112 * 112 * 3]),
        };
        assert!(matches!(
            engine.embed(&aligned),
            Err(FaceError::InvalidAlignedFace)
        ));
    }
    let blank = Zeroizing::new(vec![127; 512 * 512 * 3]);
    let blank = Frame {
        pixels: &blank,
        ..frame
    };
    assert!(engine.detect(&blank).unwrap().is_empty());
    assert!(matches!(
        extract_embedding(&mut engine, &blank, 0.9),
        Err(FaceError::PoorQuality)
    ));
    let mut pair = Zeroizing::new(Vec::with_capacity(bytes.len() * 2));
    for row in bytes.as_chunks::<{ 512 * 3 }>().0 {
        pair.extend_from_slice(row);
        pair.extend_from_slice(row);
    }
    let pair = Frame {
        width: 1024,
        height: 512,
        stride: 1024 * 3,
        format: PixelFormat::Rgb8,
        pixels: &pair,
    };
    assert_eq!(engine.detect(&pair).unwrap().len(), 2);
    assert!(matches!(
        extract_embedding(&mut engine, &pair, 0.9),
        Err(FaceError::FaceCount)
    ));
    println!(
        "pinned CPU models: single face, 128-dimensional repeatable output, blank/multiple-face rejection passed; authentication untested"
    );
}
