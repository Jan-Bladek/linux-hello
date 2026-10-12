//! Model-bound 1:1 evidence. Numerical policies require sensor-specific calibration.
use crate::{Embedding, FaceError, Template, geometry::FaceGeometry};
use serde::Serialize;

/// Safe development metadata: no descriptors, landmark coordinates or user labels.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct VerificationEvidence {
    pub template_version: u8,
    pub centroid_cosine: f32,
    pub exemplar_median: f32,
    pub selected_exemplars: usize,
    pub shape_compatible: bool,
    pub score: f32,
}

impl Template {
    pub fn verification_evidence(
        &self,
        probe: &Embedding,
        geometry: &FaceGeometry,
    ) -> Result<VerificationEvidence, FaceError> {
        if !self.strict_available() || !geometry.valid() {
            return Err(FaceError::InvalidTemplate);
        }
        // Check the model and dimension even if shape fails; a shape mismatch
        // must never conceal an incompatible embedding/model migration.
        let centroid_cosine = self.centroid.similarity(probe)?;
        let mut indices: Vec<usize> = (0..self.sample_count).collect();
        if self.version == 2 {
            // Select by measured pose, never by the largest identity score.
            indices.sort_by(|a, b| {
                let distance = |i: usize| {
                    (self.geometry[i].as_ref().expect("validated geometry").yaw() - geometry.yaw())
                        .abs()
                };
                distance(*a).total_cmp(&distance(*b)).then(a.cmp(b))
            });
            indices.truncate(3);
        }
        let shape_compatible = indices.iter().any(|&i| {
            self.geometry[i]
                .as_ref()
                .expect("validated geometry")
                .compatible(geometry)
        });
        let mut scores = indices
            .iter()
            .map(|&i| self.exemplars[i].similarity(probe))
            .collect::<Result<Vec<_>, _>>()?;
        scores.sort_by(f32::total_cmp);
        let middle = scores.len() / 2;
        let exemplar_median = if scores.len().is_multiple_of(2) {
            (scores[middle - 1] + scores[middle]) * 0.5
        } else {
            scores[middle]
        };
        Ok(VerificationEvidence {
            template_version: self.version,
            centroid_cosine,
            exemplar_median,
            selected_exemplars: indices.len(),
            shape_compatible,
            score: if shape_compatible {
                centroid_cosine.min(exemplar_median)
            } else {
                -1.0
            },
        })
    }
}

/// Explicit 1:1 sequence scoring against ONE supplied claimant's reference.
/// At least two suitable frames are required. No search over registered people.
/// Every supplied frame must pass: median voting cannot hide a different person.
pub fn sequence_score(
    template: &Template,
    frames: &[(&Embedding, &FaceGeometry)],
) -> Result<Option<f32>, FaceError> {
    if !(2..=12).contains(&frames.len()) {
        return Ok(None);
    }
    let mut minimum = 1.0_f32;
    for (embedding, geometry) in frames {
        minimum = minimum.min(template.verification_evidence(embedding, geometry)?.score);
    }
    Ok(Some(minimum))
}
