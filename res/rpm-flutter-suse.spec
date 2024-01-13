Name:       remotend
Version:    1.2.4
Release:    0
Summary:    RPM package
License:    GPL-3.0
Requires:   gtk3 libxcb1 xdotool libXfixes3 alsa-utils libXtst6 libappindicator-gtk3 libvdpau1 libva2 pam gstreamer-plugins-base gstreamer-plugin-pipewire
Provides:   libdesktop_drop_plugin.so()(64bit), libdesktop_multi_window_plugin.so()(64bit), libfile_selector_linux_plugin.so()(64bit), libflutter_custom_cursor_plugin.so()(64bit), libflutter_linux_gtk.so()(64bit), libscreen_retriever_plugin.so()(64bit), libtray_manager_plugin.so()(64bit), liburl_launcher_linux_plugin.so()(64bit), libwindow_manager_plugin.so()(64bit), libwindow_size_plugin.so()(64bit), libtexture_rgba_renderer_plugin.so()(64bit)

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

# %global __python %{__python3}

%install

mkdir -p "%{buildroot}/usr/lib/remotend" && cp -r ${HBB}/flutter/build/linux/x64/release/bundle/* -t "%{buildroot}/usr/lib/remotend"
mkdir -p "%{buildroot}/usr/bin"
install -Dm 644 $HBB/res/remotend.service -t "%{buildroot}/usr/share/remotend/files"
install -Dm 644 $HBB/res/remotend.desktop -t "%{buildroot}/usr/share/remotend/files"
install -Dm 644 $HBB/res/remotend-link.desktop -t "%{buildroot}/usr/share/remotend/files"
install -Dm 644 $HBB/res/128x128@2x.png "%{buildroot}/usr/share/icons/hicolor/256x256/apps/remotend.png"
install -Dm 644 $HBB/res/scalable.svg "%{buildroot}/usr/share/icons/hicolor/scalable/apps/remotend.svg"

%files
/usr/lib/remotend/*
/usr/share/remotend/files/remotend.service
/usr/share/icons/hicolor/256x256/apps/remotend.png
/usr/share/icons/hicolor/scalable/apps/remotend.svg
/usr/share/remotend/files/remotend.desktop
/usr/share/remotend/files/remotend-link.desktop

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
ln -s /usr/lib/remotend/remotend /usr/bin/remotend
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
    rm /usr/bin/remotend || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
