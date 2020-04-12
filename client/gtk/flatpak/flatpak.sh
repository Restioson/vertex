cargo +nightly build --release --features deploy || exit 1
flatpak-builder --repo=target/flatpak/repo target/flatpak flatpak/cf.vertex.gtk.json --force-clean || exit 1
flatpak build-bundle target/flatpak/repo target/flatpak/vertex.flatpak cf.vertex.gtk || exit 1
sudo rm -rf .flatpak-builder/
