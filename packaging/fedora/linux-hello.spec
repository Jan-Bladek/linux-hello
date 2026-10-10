# Local development RPM, not a reviewed Fedora distribution package.
# Source archive must include Cargo.lock, vendor/ and .cargo/config.toml.
%bcond_without gui

Name:           linux-hello
Version:        0.1.0
Release:        0.1.dev%{?dist}
Summary:        Local camera diagnostics for Linux Hello development
License:        Apache-2.0 AND GPL-3.0-only
Source0:        linux-hello-%{version}-dev.tar.gz

BuildRequires:  cargo >= 1.99
BuildRequires:  rust >= 1.99
BuildRequires:  gcc
BuildRequires:  clang-devel
BuildRequires:  pkgconfig
BuildRequires:  systemd-rpm-macros
BuildRequires:  dbus-daemon
%if %{with gui}
BuildRequires:  fontconfig-devel
BuildRequires:  freetype-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  wayland-devel
BuildRequires:  libX11-devel
BuildRequires:  libXcursor-devel
BuildRequires:  libXi-devel
BuildRequires:  libXrandr-devel
%endif

%description
Development preview containing real V4L2 enumeration and bounded capture
diagnostics. Face authentication is unavailable. This package does not install
a PAM module, alter system authentication, or enable any service.

%package daemon
Summary:        Read-only Linux Hello diagnostic D-Bus service
License:        Apache-2.0
Requires:       dbus
Requires:       systemd
Requires(pre):  shadow-utils
Requires(post): systemd
Requires(postun): systemd

%description daemon
Optional unprivileged diagnostic service. It has no camera, template or key
access and cannot authenticate users. Installation never starts or enables it.

%if %{with gui}
%package gui
Summary:        Native Slint camera preview and Linux Hello diagnostics
License:        GPL-3.0-only
Requires:       %{name}%{?_isa} = %{version}-%{release}
Requires:       fontconfig
Requires:       libxkbcommon
Requires:       wayland-libs

%description gui
Native settings and explicit camera-preview application. Security capabilities
are reported honestly; face enrollment and authentication are unavailable.
%endif

%prep
%autosetup -n linux-hello-%{version}-dev
# The source-generation step vendors the locked dependencies. Never fetch crates
# or model weights inside the RPM build, and never invoke rustup in a buildroot.
test -d vendor
test -f .cargo/config.toml
test -f Cargo.lock

%build
export CARGO_NET_OFFLINE=true
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_DEBUG=1
export CARGO_PROFILE_RELEASE_STRIP=false
cargo build --frozen --release -p hello-cli -p hello-daemon
%if %{with gui}
cargo build --frozen --release -p hello-settings
%endif

%check
export CARGO_NET_OFFLINE=true
cargo test --frozen -p hello-core -p hello-camera -p hello-face -p hello-storage -p hello-ipc -p hello-fingerprint -p hello-pam
dbus-run-session -- cargo test --frozen -p hello-daemon isolated_bus_identity_concurrency_and_denial -- --include-ignored

%install
install -Dpm0755 target/release/hello %{buildroot}%{_bindir}/hello
install -Dpm0755 target/release/hello-daemon %{buildroot}%{_libexecdir}/hello-daemon
install -Dpm0644 packaging/systemd/linux-hello.service %{buildroot}%{_unitdir}/linux-hello.service
install -Dpm0644 packaging/dbus/org.linuxhello.Daemon1.conf %{buildroot}%{_datadir}/dbus-1/system.d/org.linuxhello.Daemon1.conf
%if %{with gui}
install -Dpm0755 target/release/hello-settings %{buildroot}%{_bindir}/hello-settings
install -Dpm0644 packaging/fedora/org.linuxhello.Settings.desktop %{buildroot}%{_datadir}/applications/org.linuxhello.Settings.desktop
%endif
# Deliberately do not install polkit, authselect, PAM, or D-Bus activation files.

%pre daemon
getent group linux-hello >/dev/null || groupadd -r linux-hello || exit 1
getent passwd linux-hello >/dev/null || useradd -r -g linux-hello -d / -s /sbin/nologin -c "Linux Hello diagnostics" linux-hello || exit 1
exit 0

%post daemon
# Do not use systemd preset/enable macros: installation must be inert.
systemctl daemon-reload >/dev/null 2>&1 || :

%preun daemon
if [ "$1" -eq 0 ]; then
    systemctl stop linux-hello.service >/dev/null 2>&1 || :
fi

%postun daemon
systemctl daemon-reload >/dev/null 2>&1 || :

%files
%license LICENSE
%doc README.md SECURITY.md CONTRIBUTING.md docs
%{_bindir}/hello

%files daemon
%license LICENSE
%{_libexecdir}/hello-daemon
%{_unitdir}/linux-hello.service
%{_datadir}/dbus-1/system.d/org.linuxhello.Daemon1.conf

%if %{with gui}
%files gui
%license LICENSE LICENSE-GPL-3.0
%{_bindir}/hello-settings
%{_datadir}/applications/org.linuxhello.Settings.desktop
%endif

%changelog
* Sat Oct 10 2026 Linux Hello contributors - 0.1.0-0.1.dev
- Initial diagnostic preview; no enabled authentication integration.
