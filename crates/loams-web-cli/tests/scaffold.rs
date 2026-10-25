use loams_web_cli::{ScaffoldOptions, scaffold};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "loams-web-scaffold-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn options(&self) -> ScaffoldOptions {
        ScaffoldOptions {
            directory: self.0.join("hello"),
            name: "hello-app".into(),
            framework_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../loams-web"),
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn writes_local_standalone_typed_server_and_refuses_overwrite() {
    let temp = Temp::new();
    let options = temp.options();
    let destination = scaffold(&options).unwrap();
    let manifest = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"hello-app\""));
    assert!(manifest.contains("[workspace]"));
    assert!(
        manifest.contains(
            &options
                .framework_path
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string()
        )
    );
    let source = fs::read_to_string(destination.join("src/main.rs")).unwrap();
    assert!(source.contains("Procedure::<Hello, Greeting>"));
    assert!(source.contains("ServerConfig::default()"));
    assert!(source.contains("/openapi.json"));
    assert!(source.contains("127.0.0.1:8080"));
    assert!(
        fs::read_to_string(destination.join("README.md"))
            .unwrap()
            .contains("unpublished")
    );
    assert!(scaffold(&options).is_err());
    assert_eq!(
        manifest,
        fs::read_to_string(destination.join("Cargo.toml")).unwrap()
    );
}

#[test]
fn accepts_checkout_root_and_resolves_facade() {
    let temp = Temp::new();
    let mut options = temp.options();
    options.framework_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(scaffold(&options).is_ok());
    // The generated dependency always resolves to the facade, never a workspace manifest.
    let text = fs::read_to_string(options.directory.join("Cargo.toml")).unwrap();
    assert!(text.contains("crates/loams-web\""));
}

#[test]
fn rejects_names_and_missing_framework_before_creating_destination() {
    let temp = Temp::new();
    for name in [
        "",
        "../escape",
        "Upper",
        "bad_name",
        "bad\nname",
        "std",
        "con",
        "com1",
        "trailing-",
        "fn",
        "gen",
        "loams-web",
        "loams-web-http",
    ] {
        let mut options = temp.options();
        options.name = name.into();
        assert!(scaffold(&options).is_err(), "accepted {name:?}");
        assert!(!options.directory.exists());
    }
    let mut options = temp.options();
    options.framework_path = temp.0.clone();
    assert!(scaffold(&options).is_err());
    assert!(!options.directory.exists());
}

#[cfg(unix)]
#[test]
fn refuses_dangling_destination_symlink_without_touching_target() {
    let temp = Temp::new();
    let options = temp.options();
    let target = temp.0.join("absent");
    std::os::unix::fs::symlink(&target, &options.directory).unwrap();
    assert!(scaffold(&options).is_err());
    assert!(!target.exists());
    assert!(
        fs::symlink_metadata(&options.directory)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn binary_accepts_new_command_and_preserves_generation_usage() {
    let temp = Temp::new();
    let options = temp.options();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_loams-web-cli"))
        .arg("new")
        .arg(&options.directory)
        .arg("--name")
        .arg(&options.name)
        .arg("--framework-path")
        .arg(&options.framework_path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_loams-web-cli"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--write|--check"));
}
