#!/bin/bash

# Build
cargo +nightly build --release || exit 1

CRATE=$(pwd)
cd target/release/

# Clean old build files
rm Vertex.AppImage
rm -rf Vertex.AppDir
mkdir -p Vertex.AppDir

# Copy files
cp -r $CRATE/res Vertex.AppDir
cp vertex_client_gtk Vertex.AppDir/AppRun
cp $CRATE/vertex.desktop Vertex.AppDir
cd Vertex.AppDir
cp res/icon.png vertex_client_gtk.png

# Build the app image
cd $CRATE
./appimagetool-x86_64.AppImage target/release/Vertex.AppDir target/release/Vertex.AppImage
