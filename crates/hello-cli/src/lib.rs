#[cfg(all(feature = "face-opencv", target_os = "linux"))]
pub mod face_worker;

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
pub mod evaluation;

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
pub mod diagnostics;

#[cfg(target_os = "linux")]
pub mod account;
pub mod health;
