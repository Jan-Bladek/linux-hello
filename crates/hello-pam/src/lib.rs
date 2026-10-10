//! PAM integration readiness, not an installable PAM module.
//!
//! No `pam_sm_*` exports are built until the privileged protocol, PAD and a
//! disposable Fedora VM recovery test have been reviewed. Nothing in this crate
//! returns PAM_SUCCESS or modifies system authentication.
#![forbid(unsafe_code)]

pub const IMPLEMENTED: bool = false;

pub fn blockers() -> &'static [&'static str] {
    &[
        "No evaluated face model or presentation attack detector",
        "No privileged enrollment or verification IPC protocol",
        "No PAM adapter exported or installed",
        "Fedora VM password fallback and recovery not validated",
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn pam_remains_unavailable() {
        const { assert!(!super::IMPLEMENTED) };
        assert!(!hello_ipc::Capabilities::default().pam_module);
        assert_eq!(super::blockers().len(), 4);
    }
}
