Name:       remotend
Version:    1.2.4
Release:    0
Summary:    RPM package
License:    GPL-3.0
Requires:   gtk3 libxcb libxdo libXfixes alsa-lib libappindicator libvdpau1 libva2 pam gstreamer1-plugins-base

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/lib/remotend/
mkdir -p %{buildroot}/usr/share/remotend/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/remotend %{buildroot}/usr/bin/remotend
install $HBB/libsciter-gtk.so %{buildroot}/usr/lib/remotend/libsciter-gtk.so
install $HBB/res/remotend.service %{buildroot}/usr/share/remotend/files/
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/remotend.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/remotend.svg
install $HBB/res/remotend.desktop %{buildroot}/usr/share/remotend/files/
install $HBB/res/remotend-link.desktop %{buildroot}/usr/share/remotend/files/

%files
/usr/bin/remotend
/usr/lib/remotend/libsciter-gtk.so
/usr/share/remotend/files/remotend.service
/usr/share/icons/hicolor/256x256/apps/remotend.png
/usr/share/icons/hicolor/scalable/apps/remotend.svg
/usr/share/remotend/files/remotend.desktop
/usr/share/remotend/files/remotend-link.desktop
/usr/share/remotend/files/__pycache__/*

%changelog
# let's skip this for now

# https://www.cnblogs.com/xingmuxin/p/8990255.html
%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop remotend || true
  ;;
esac

%post
cp /usr/share/remotend/files/remotend.service /etc/systemd/system/remotend.service
cp /usr/share/remotend/files/remotend.desktop /usr/share/applications/
cp /usr/share/remotend/files/remotend-link.desktop /usr/share/applications/
systemctl daemon-reload
systemctl enable remotend
systemctl start remotend
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop remotend || true
    systemctl disable remotend || true
    rm /etc/systemd/system/remotend.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/share/applications/remotend.desktop || true
    rm /usr/share/applications/remotend-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
