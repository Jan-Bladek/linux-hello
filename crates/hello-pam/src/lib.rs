//! Default builds report diagnostic readiness only. The explicit Linux
//! `sudo-pam` feature exports a sudo-only experimental PAM adapter backed by an
//! installed root-owned verifier and protected, UID-bound enrollment. It does
//! not advertise qualified PAD or enable the diagnostic D-Bus daemon for auth.
//! `kde-pam` also permits opted-in root Plasma Login/SDDM helpers and a self-only
//! unprivileged KScreenLocker, Hyprlock and Noctalia-v5 login-stack callers via
//! the self-only setuid verification launcher. Root login/TTY callers are excluded.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(all(feature = "privileged", target_os = "linux"))]
pub mod boundary;
#[cfg(all(feature = "sudo-pam", target_os = "linux"))]
mod sudo;

pub const IMPLEMENTED: bool = cfg!(all(feature = "sudo-pam", target_os = "linux"));

pub fn blockers() -> &'static [&'static str] {
    &[
        "Representative NIR FAR/FRR and presentation attacks are not validated",
        "General-purpose privileged authentication IPC remains unavailable",
        "sudo/KDE adapters require explicit features, root installation and enrollment",
        "Production security qualification, sshd and disposable-VM recovery remain unavailable",
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn pam_remains_unavailable() {
        assert_eq!(
            super::IMPLEMENTED,
            cfg!(all(feature = "sudo-pam", target_os = "linux"))
        );
        assert!(!hello_ipc::Capabilities::default().pam_module);
        assert_eq!(super::blockers().len(), 4);
    }
}
