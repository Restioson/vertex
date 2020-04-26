#[cfg(not(windows))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}

#[cfg(windows)]
fn main() {
    use std::env;
    use std::path::PathBuf;
    use path_slash::PathBufExt;

    println!("cargo:rerun-if-changed=res/icon.ico");

    let out_dir = env::var("OUT_DIR").unwrap_or(".".to_owned());
    let out_dir = PathBuf::from(out_dir).to_slash().unwrap();
    let out_dir = format!("{}/", out_dir);

    winres::WindowsResource::new()
        .set_output_directory(&out_dir)
        .set_icon("res/icon.ico")
        .compile()
        .expect("failed to compile logo");
}
