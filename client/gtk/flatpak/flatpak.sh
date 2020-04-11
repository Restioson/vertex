sudo rm -rf .flatpak-builder/
cargo +nightly build --release || exit 1
flatpak-builder --repo=target/flatpak/repo target/flatpak flatpak/cf.vertex.gtk.json --force-clean || exit 1
flatpak build-bundle target/flatpak/repo target/flatpak/vertex.flatpak cf.vertex.gtk || exit 1
