flatpak-builder --repo=target/armv7-unknown-linux-gnueabihf/flatpak/repo target/armv7-unknown-linux-gnueabihf/flatpak flatpak/arm7/cf.vertex.gtk.json --force-clean --arch=arm --disable-rofiles-fuse || exit 1
flatpak build-bundle target/armv7-unknown-linux-gnueabihf/flatpak/repo target/armv7-unknown-linux-gnueabihf/flatpak/vertex.flatpak cf.vertex.gtk --arch=arm || exit 1
sudo rm -rf .flatpak-builder/
