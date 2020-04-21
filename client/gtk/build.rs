#[cfg(windows)]
type StdResult<T> = Result<T, Box<dyn std::error::Error>>;

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use std::env;
    use std::path::PathBuf;
    use path_slash::PathBufExt;

    if let Err(err) = generate_wix_files() {
        println!("failed to generate wix files: {:?}", err);
    }

    let out_dir = env::var("OUT_DIR").unwrap_or(".".to_owned());
    let out_dir = PathBuf::from(out_dir).to_slash().unwrap();
    let out_dir = format!("{}/", out_dir);

    winres::WindowsResource::new()
        .set_output_directory(&out_dir)
        .set_icon("res/icon.ico")
        .compile()
        .expect("failed to compile logo");
}

#[cfg(windows)]
fn generate_wix_files() -> StdResult<()> {
    use std::fs::{self, File};
    use std::path::PathBuf;
    use walkdir::WalkDir;
    use path_slash::PathExt;

    const RES_PATH: &str = "res";

    let mut resources = wix::Files {
        dir_ref_name: "ResourceDirRef".to_owned(),
        component_group_name: "ResourceGroupId".to_owned(),
        root: wix::Directory::default(),
    };

    for entry in WalkDir::new(RES_PATH) {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            let path = entry.path();

            let mut directory = &mut resources.root;

            let components: Vec<_> = path.components().collect();

            // first component is res directory and last component is file name
            let components = &components[1..components.len() - 1];

            for component in components {
                let component = component.as_os_str().to_string_lossy().to_string();
                directory = directory.sub_directories.entry(component).or_default();
            }

            directory.components.push(wix::Component {
                path: format!("SourceDir/{}", path.to_slash_lossy())
            })
        }
    }

    let wix_generated = PathBuf::from("wix/generated");
    fs::create_dir_all(&wix_generated)?;

    let resources_file = File::create(wix_generated.join("resources.wxs"))?;
    wix::write(resources_file, resources)?;

    const MINGW_BIN: &str = "C:\\msys64\\mingw64\\bin";
    const BINARIES: [&str; 45] = [
        "libgdk-3-0.dll",
        "libcairo-2.dll",
        "libcairo-gobject-2.dll",
        "libiconv-2.dll",
        "libgdk_pixbuf-2.0-0.dll",
        "libgio-2.0-0.dll",
        "libglib-2.0-0.dll",
        "libgobject-2.0-0.dll",
        "libgtk-3-0.dll",
        "libopenal-1.dll",
        "libpango-1.0-0.dll",
        "libsndfile-1.dll",
        "libfontconfig-1.dll",
        "libfreetype-6.dll",
        "libpixman-1-0.dll",
        "zlib1.dll",
        "libpng16-16.dll",
        "libgmodule-2.0-0.dll",
        "libintl-8.dll",
        "libpcre-1.dll",
        "libffi-6.dll",
        "libepoxy-0.dll",
        "libpangocairo-1.0-0.dll",
        "libpangowin32-1.0-0.dll",
        "libatk-1.0-0.dll",
        "libharfbuzz-0.dll",
        "libpangoft2-1.0-0.dll",
        "libFLAC-8.dll",
        "libogg-0.dll",
        "libspeex-1.dll",
        "libvorbis-0.dll",
        "libvorbisenc-2.dll",
        "libexpat-1.dll",
        "libbz2-1.dll",
        "libgraphite2.dll",
        "libthai-0.dll",
        "libfribidi-0.dll",
        "libdatrie-1.dll",
        "librsvg-2-2.dll",
        "libxml2-2.dll",
        "libcroco-0.6-3.dll",
        "liblzma-5.dll",
        "libgcc_s_seh-1.dll",
        "libwinpthread-1.dll",
        "libstdc++-6.dll",
    ];

    let mut binaries = wix::Files {
        dir_ref_name: "BinaryDirRef".to_owned(),
        component_group_name: "BinaryGroupId".to_owned(),
        root: wix::Directory::default(),
    };

    for &binary in BINARIES.iter() {
        binaries.root.components.push(wix::Component {
            path: format!("{}\\{}", MINGW_BIN, binary),
        });
    }

    let binaries_file = File::create("wix/generated/binaries.wxs")?;
    wix::write(binaries_file, binaries)?;

    Ok(())
}

#[cfg(windows)]
mod wix {
    use std::io;
    use std::collections::HashMap;

    use xml::writer::{EmitterConfig, EventWriter, Result, XmlEvent};

    pub struct Files {
        pub dir_ref_name: String,
        pub component_group_name: String,
        pub root: Directory,
    }

    #[derive(Default)]
    pub struct Directory {
        pub sub_directories: HashMap<String, Directory>,
        pub components: Vec<Component>,
    }

    pub struct Component {
        pub path: String,
    }

    fn random_id() -> String {
        use rand::Rng;

        let mut rng = rand::thread_rng();
        (0..32)
            .map(|_| rng.gen_range(b'a', b'z' + 1) as char)
            .collect()
    }

    pub fn write<W: io::Write>(write: W, files: Files) -> Result<()> {
        let mut w: EventWriter<W> = EmitterConfig::new()
            .perform_indent(true)
            .create_writer(write);

        w.write(
            XmlEvent::start_element("Wix")
                .attr("xmlns", "http://schemas.microsoft.com/wix/2006/wi")
        )?;

        {
            let mut component_ids = Vec::new();

            w.write(XmlEvent::start_element("Fragment"))?;
            {
                write_components(&mut w, &files, &mut component_ids)?;
            }
            w.write(XmlEvent::end_element())?;

            w.write(XmlEvent::start_element("Fragment"))?;
            {
                write_group(&mut w, &files.component_group_name, component_ids)?;
            }
            w.write(XmlEvent::end_element())?;
        }

        w.write(XmlEvent::end_element())?;

        Ok(())
    }

    fn write_components<W: io::Write>(w: &mut EventWriter<W>, files: &Files, component_ids: &mut Vec<String>) -> Result<()> {
        w.write(
            XmlEvent::start_element("DirectoryRef")
                .attr("Id", &files.dir_ref_name)
        )?;

        let root = &files.root;
        for component in &root.components {
            let id = write_file_component(w, component)?;
            component_ids.push(id);
        }

        for (name, directory) in &root.sub_directories {
            write_directory(w, name, directory, component_ids)?;
        }

        w.write(XmlEvent::end_element())?;

        Ok(())
    }

    fn write_directory<W: io::Write>(w: &mut EventWriter<W>, name: &str, directory: &Directory, component_ids: &mut Vec<String>) -> Result<()> {
        w.write(
            XmlEvent::start_element("Directory")
                .attr("Id", &random_id())
                .attr("Name", name)
        )?;

        for component in &directory.components {
            let id = write_file_component(w, component)?;
            component_ids.push(id);
        }

        for (name, directory) in &directory.sub_directories {
            write_directory(w, name, directory, component_ids)?;
        }

        w.write(XmlEvent::end_element())?;

        Ok(())
    }

    fn write_file_component<W: io::Write>(w: &mut EventWriter<W>, component: &Component) -> Result<String> {
        let guid = uuid::Uuid::new_v4();
        let component_id = random_id();

        w.write(
            XmlEvent::start_element("Component")
                .attr("Id", &component_id)
                .attr("Win64", "yes")
                .attr("Guid", &format!("{{{}}}", guid.to_hyphenated()))
        )?;

        {
            w.write(
                XmlEvent::start_element("File")
                    .attr("Id", &random_id())
                    .attr("KeyPath", "yes")
                    .attr("Source", &component.path)
            )?;

            w.write(XmlEvent::end_element())?;
        }

        w.write(XmlEvent::end_element())?;

        Ok(component_id)
    }

    fn write_group<W: io::Write>(w: &mut EventWriter<W>, name: &str, component_ids: Vec<String>) -> Result<()> {
        w.write(
            XmlEvent::start_element("ComponentGroup")
                .attr("Id", name)
        )?;

        for id in component_ids {
            w.write(
                XmlEvent::start_element("ComponentRef")
                    .attr("Id", &id)
            )?;

            w.write(XmlEvent::end_element())?;
        }

        w.write(XmlEvent::end_element())?;

        Ok(())
    }
}
