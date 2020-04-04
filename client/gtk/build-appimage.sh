#!/bin/bash

rm Vertex.AppImage
rm -rf Vertex.AppDir
cargo +nightly build --release
mkdir -p Vertex.AppDir
cp -r res Vertex.AppDir

cp target/release/vertex_client_gtk Vertex.AppDir/AppRun

cd Vertex.AppDir
mv res/icon.svg vertex_client_gtk.png

echo '[Desktop Entry]' >> vertex.desktop
echo 'Name=Vertex' >> vertex.desktop
echo 'Exec=Vertex' >> vertex.desktop
echo 'Icon=vertex_client_gtk' >> vertex.desktop
echo 'Type=Application' >> vertex.desktop
echo 'Categories=Chat;' >> vertex.desktop

cd ..
./appimagetool-x86_64.AppImage Vertex.AppDir Vertex.AppImage
