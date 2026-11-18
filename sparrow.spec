Name:           sparrow-git
Version:        0.4.6.{{{ git_dir_version }}}
Release:        1
Summary:        Dart and Flutter IDE
License:        Apache-2.0
URL:            https://github.com/infinite-dev22/sparrow

VCS:            {{{ git_dir_vcs }}}
Source:        	{{{ git_dir_pack }}}

BuildRequires:  cargo libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf libxkbcommon-x11-devel

%description
Sparrow is a native Dart and Flutter IDE with SDK analysis, testing, debugging,
project tools, an integrated terminal, and local appearance themes.

%prep
{{{ git_dir_setup_macro }}}
cargo fetch --locked

%build
cargo build --profile release-lto --package sparrow --bin sparrow --frozen

%install
install -Dm755 target/release-lto/sparrow %{buildroot}%{_bindir}/sparrow
install -Dm644 extra/linux/dev.sparrow.sparrow.desktop %{buildroot}/usr/share/applications/dev.sparrow.sparrow.desktop
install -Dm644 extra/linux/dev.sparrow.sparrow.metainfo.xml %{buildroot}/usr/share/metainfo/dev.sparrow.sparrow.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/dev.sparrow.sparrow.png

%files
%license LICENSE*
%doc *.md
%{_bindir}/sparrow
/usr/share/applications/dev.sparrow.sparrow.desktop
/usr/share/metainfo/dev.sparrow.sparrow.metainfo.xml
/usr/share/pixmaps/dev.sparrow.sparrow.png

%changelog
* Mon Jan 01 2024 Jakub Panek
- See full changelog on GitHub
