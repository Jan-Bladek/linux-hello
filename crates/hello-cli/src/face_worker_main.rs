#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = hello_cli::face_worker::run_worker() {
        // Parent receives a bounded failure, never native stderr or image data.
        eprintln!("inference worker failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    std::process::exit(1);
}
