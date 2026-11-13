# Local binary packaging: scripts/package-rpm.sh builds the locked Rust source
# and supplies Source0. This is not a distribution/source RPM recipe.
%global debug_package %{nil}
Name:           jaywalk
Version:        %{app_version}
Release:        %{app_release}%{?dist}
Summary:        Native Git and Jujutsu diff viewer
License:        GPL-3.0-only AND Apache-2.0 AND MIT AND BSD-2-Clause AND BSD-3-Clause AND ISC AND Zlib AND MPL-2.0 AND Unicode-3.0 AND CC0-1.0 AND 0BSD
URL:            https://github.com/sorahn/jaywalk
Source0:        jaywalk-%{version}-linux-%{_arch}.tar.gz
Requires:       git
Requires:       xdg-utils
Requires:       fontconfig
# GPUI's system Wayland backend uses dlopen, so ELF auto-requires cannot find it.
%if 0%{?suse_version}
Requires:       libvulkan1
Requires:       libwayland-client0%{?_isa}
%else
Requires:       vulkan-loader
Requires:       libwayland-client%{?_isa}
%endif

%description
Jaywalk is a native Rust/GPUI viewer for Git and Jujutsu comparisons.
Jujutsu repositories additionally require jj 0.47 or newer on PATH.
Built locally with the checkout's locked Cargo dependencies.

%prep
%setup -q

%install
install -Dm755 jaywalk %{buildroot}%{_bindir}/jaywalk
install -Dm755 jw %{buildroot}%{_bindir}/jw
install -Dm644 io.github.sorahn.jaywalk.desktop %{buildroot}%{_datadir}/applications/io.github.sorahn.jaywalk.desktop
install -Dm644 io.github.sorahn.jaywalk.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.sorahn.jaywalk.svg

%files
%license LICENSE LICENSE-APACHE
%doc README.md
%doc DEPENDENCIES.txt
%doc VENDORED.md
%{_bindir}/jaywalk
%{_bindir}/jw
%{_datadir}/applications/io.github.sorahn.jaywalk.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.sorahn.jaywalk.svg
