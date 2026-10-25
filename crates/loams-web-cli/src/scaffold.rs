//! Local unpublished-framework project scaffolding, without invoking a shell.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// A destination whose parent already exists, a Cargo package name, and the
/// existing `crates/loams-web` facade directory in a local framework checkout.
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    pub directory: PathBuf,
    pub name: String,
    pub framework_path: PathBuf,
}

#[derive(Debug)]
pub enum ScaffoldError {
    InvalidName,
    InvalidDestination,
    InvalidFramework,
    Io(io::Error),
}
impl std::fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => f.write_str("Package name must start with a lowercase ASCII letter, contain only lowercase letters, digits or hyphens, and be at most 64 characters; reserved names are refused"),
            Self::InvalidDestination => f.write_str("Destination must name a new directory with an existing parent"),
            Self::InvalidFramework => f.write_str("Framework path must name the local loams-web facade crate, with Cargo.toml and src/lib.rs, using a UTF-8 path"),
            Self::Io(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ScaffoldError {}
impl From<io::Error> for ScaffoldError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.ends_with('-')
        && ![
            "std",
            "core",
            "alloc",
            "test",
            "proc-macro",
            "loams-web",
            "loams-web-core",
            "loams-web-http",
            "loams-web-openapi",
            "loams-web-cli",
            "con",
            "prn",
            "aux",
            "nul",
            "as",
            "break",
            "const",
            "continue",
            "crate",
            "else",
            "enum",
            "extern",
            "false",
            "fn",
            "for",
            "if",
            "impl",
            "in",
            "let",
            "loop",
            "match",
            "mod",
            "move",
            "mut",
            "pub",
            "ref",
            "return",
            "self",
            "static",
            "struct",
            "super",
            "trait",
            "true",
            "type",
            "unsafe",
            "use",
            "where",
            "while",
            "async",
            "await",
            "dyn",
            "abstract",
            "become",
            "box",
            "do",
            "final",
            "gen",
            "macro",
            "override",
            "priv",
            "try",
            "typeof",
            "unsized",
            "virtual",
            "yield",
        ]
        .contains(&name)
        && !(name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && name.as_bytes()[3].is_ascii_digit())
}

fn facade_manifest(text: &str) -> bool {
    let mut package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            package = line == "[package]";
        }
        if package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
            && value.split('#').next().unwrap_or("").trim() == "\"loams-web\""
        {
            return true;
        }
    }
    false
}

/// Create a minimal runnable application. Existing destinations, including
/// dangling symlinks, are refused. No commands are executed. Returns the
/// canonical destination. On write failure only files created here are removed.
pub fn scaffold(options: &ScaffoldOptions) -> Result<PathBuf, ScaffoldError> {
    if !valid_name(&options.name) {
        return Err(ScaffoldError::InvalidName);
    }
    let leaf = options
        .directory
        .file_name()
        .ok_or(ScaffoldError::InvalidDestination)?;
    let parent = options
        .directory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination = parent.canonicalize()?.join(leaf);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination already exists; no files were changed",
            )
            .into());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let supplied = options.framework_path.canonicalize()?;
    let framework = if supplied.join("crates/loams-web/Cargo.toml").is_file() {
        supplied.join("crates/loams-web").canonicalize()?
    } else {
        supplied
    };
    let framework_text = framework.to_str().ok_or(ScaffoldError::InvalidFramework)?;
    if !framework.join("src/lib.rs").is_file()
        || !facade_manifest(&fs::read_to_string(framework.join("Cargo.toml"))?)
    {
        return Err(ScaffoldError::InvalidFramework);
    }
    let path_literal = serde_json::to_string(framework_text).expect("UTF-8 string serializes");
    let manifest = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"
publish = false

# Standalone even when created inside another Cargo workspace.
[workspace]

[dependencies]
loams-web = {{ path = {} }}
serde = {{ version = "1", features = ["derive"] }}
schemars = {{ version = "1", features = ["derive"] }}
tokio = {{ version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }}
axum = "0.8"
"#,
        options.name, path_literal
    );
    let readme = format!(
        r#"# {}

A typed POST `/hello` procedure using a local Loams Web checkout. Its
`loams-web` dependency points at `{}`; keep that checkout available or update
the path in Cargo.toml when moving the project. The framework is unpublished.

Run `cargo run` (or `cargo run --offline` when dependencies are cached).
`LOAMS_WEB_ADDR` defaults to `127.0.0.1:8080`; use another socket address to
change it. Press Ctrl-C to stop accepting connections and drain requests.

```sh
curl -H 'content-type: application/json' -d '{{"name":"Ada"}}' http://127.0.0.1:8080/hello
curl http://127.0.0.1:8080/openapi.json
```

The same procedure descriptor drives request validation, the handler and
OpenAPI. Input accepts a name of 1–100 characters and refuses unknown fields.
"#,
        options.name, framework_text
    );
    let files = [
        ("Cargo.toml", manifest.as_str()),
        ("README.md", readme.as_str()),
        (".gitignore", "/target/\n"),
        ("src/main.rs", MAIN),
    ];
    fs::create_dir(&destination)?;
    let mut created = Vec::new();
    let written = (|| -> Result<(), io::Error> {
        fs::create_dir(destination.join("src"))?;
        for (relative, text) in files {
            let path = destination.join(relative);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            created.push(path);
            file.write_all(text.as_bytes())?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        for path in created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(destination.join("src"));
        let _ = fs::remove_dir(&destination);
        return Err(error.into());
    }
    Ok(destination)
}

const MAIN: &str = r#"use loams_web::{HttpMethod, Procedure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Hello {
    #[schemars(length(min = 1, max = 100))]
    name: String,
}

#[derive(Serialize, JsonSchema)]
struct Greeting {
    greeting: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hello = Procedure::<Hello, Greeting>::builder("hello")
        .http(HttpMethod::Post, "/hello")
        .summary("Greet someone by name")
        .handler(|_context, input| async move {
            Ok(Greeting { greeting: format!("Hello, {}!", input.name) })
        })
        .build()?;
    let document = loams_web::openapi::generate([hello.descriptor()], "Hello", "0.1.0")?;
    let router = loams_web::http::HttpAdapter::new()
        .procedure(hello)?
        .router()?
        .route("/openapi.json", axum::routing::get(move || async move { axum::Json(document) }));
    let address = std::env::var("LOAMS_WEB_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Listening on http://{}", listener.local_addr()?);
    loams_web::http::server::Server::new(loams_web::http::server::ServerConfig::default())?
        .serve(listener, router, async { let _ = tokio::signal::ctrl_c().await; })
        .await?;
    Ok(())
}
"#;
