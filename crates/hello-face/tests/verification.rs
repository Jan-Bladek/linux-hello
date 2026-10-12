//! Synthetic arithmetic regressions; not an accuracy or NIR benchmark.
use hello_face::{
    Detection, Embedding, Enrollment, FaceError, QualityMetrics, Template, geometry::FaceGeometry,
    verification::sequence_score,
};
fn embedding(angle: f32) -> Embedding {
    Embedding::new([1; 32], vec![angle.cos(), angle.sin()]).unwrap()
}
fn shape(yaw: f32) -> FaceGeometry {
    FaceGeometry::measure(&Detection {
        bounds: [0.0, 0.0, 200.0, 240.0],
        landmarks: [
            [50.0, 60.0],
            [150.0, 60.0],
            [100.0 + yaw * 100.0, 120.0],
            [65.0, 180.0],
            [135.0, 180.0],
        ],
        confidence: 0.99,
    })
    .unwrap()
}
fn quality() -> QualityMetrics {
    QualityMetrics {
        mean_intensity: 0.5,
        contrast: 0.1,
        edge_energy: 0.1,
    }
}
fn reference() -> Template {
    let mut e = Enrollment::new_pose_aware(0.8).unwrap();
    for (i, (angle, yaw)) in [
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.45, 0.16),
        (-0.45, -0.16),
        (-0.45, -0.16),
        (-0.45, -0.16),
        (-0.45, -0.16),
    ]
    .into_iter()
    .enumerate()
    {
        e.add_with_geometry([i as u8; 32], embedding(angle), 1, quality(), shape(yaw))
            .unwrap();
    }
    e.finish().unwrap()
}
#[test]
fn opposite_pose_samples_do_not_dominate_the_v2_pose_median() {
    let template = reference();
    let probe = embedding(0.45);
    let geometry = shape(0.16);
    let evidence = template.verification_evidence(&probe, &geometry).unwrap();
    assert_eq!(template.version(), 2);
    assert_eq!(evidence.selected_exemplars, 3);
    assert!(evidence.score >= 0.8);
    let mut wire = serde_json::to_value(&template).unwrap();
    wire["version"] = 1.into();
    let legacy = Template::from_bytes(&serde_json::to_vec(&wire).unwrap()).unwrap();
    assert!(legacy.strict_similarity(&probe, &geometry).unwrap() < 0.8);
}
#[test]
fn sequence_requires_multiple_frames_and_cannot_hide_an_unknown_face() {
    let template = reference();
    let known = embedding(0.0);
    let unknown = embedding(1.2);
    let geometry = shape(0.0);
    assert!(
        sequence_score(&template, &[(&known, &geometry)])
            .unwrap()
            .is_none()
    );
    assert!(
        sequence_score(&template, &[(&known, &geometry), (&known, &geometry)])
            .unwrap()
            .unwrap()
            >= 0.8
    );
    assert!(
        sequence_score(&template, &[(&known, &geometry), (&unknown, &geometry)])
            .unwrap()
            .unwrap()
            < 0.8
    );
    let wrong = Embedding::new([2; 32], vec![1.0, 0.0]).unwrap();
    assert_eq!(
        sequence_score(&template, &[(&known, &geometry), (&wrong, &geometry)]).err(),
        Some(FaceError::IncompatibleEmbedding)
    );
}
#[test]
fn v2_template_corruption_and_incompatible_migration_fail_closed() {
    let template = reference();
    let bytes = template.to_bytes().unwrap();
    assert_eq!(Template::from_bytes(&bytes).unwrap().version(), 2);
    for field in ["geometry", "exemplars"] {
        let mut wire = serde_json::to_value(&template).unwrap();
        wire.as_object_mut().unwrap().remove(field);
        assert!(Template::from_bytes(&serde_json::to_vec(&wire).unwrap()).is_err());
    }
    let mut wire = serde_json::to_value(&template).unwrap();
    wire["centroid"]["values"] = serde_json::json!([0.0, 1.0]);
    assert!(Template::from_bytes(&serde_json::to_vec(&wire).unwrap()).is_err());
    let mut wire = serde_json::to_value(&template).unwrap();
    wire["version"] = 99.into();
    assert!(Template::from_bytes(&serde_json::to_vec(&wire).unwrap()).is_err());
}
#[test]
fn new_enrollment_checks_frontal_anchors_and_does_not_mix_users_or_models() {
    let mut e = Enrollment::new_pose_aware(0.8).unwrap();
    e.add_with_geometry([0; 32], embedding(0.0), 1, quality(), shape(0.0))
        .unwrap();
    assert_eq!(
        e.add_with_geometry([1; 32], embedding(0.0), 1, quality(), shape(0.16))
            .err(),
        Some(FaceError::EnrollmentPose)
    );
    assert_eq!(e.sample_count(), 1);
    for i in 1..3 {
        e.add_with_geometry([i; 32], embedding(0.0), 1, quality(), shape(0.0))
            .unwrap();
    }
    e.add_with_geometry([3; 32], embedding(0.45), 1, quality(), shape(0.16))
        .unwrap();
    e.add_with_geometry([4; 32], embedding(-0.45), 1, quality(), shape(-0.16))
        .unwrap();
    assert_eq!(
        e.add_with_geometry([5; 32], embedding(1.2), 1, quality(), shape(0.0))
            .err(),
        Some(FaceError::InconsistentSample)
    );
    assert_eq!(e.sample_count(), 5);
    let bad = Embedding::new([2; 32], vec![1.0, 0.0]).unwrap();
    assert_eq!(
        e.add_with_geometry([5; 32], bad, 1, quality(), shape(0.0))
            .err(),
        Some(FaceError::IncompatibleEmbedding)
    );
}
