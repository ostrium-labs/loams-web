use loams_web_cli::{ScaffoldOptions, check_client, generate_typescript, scaffold, write_client};
use loams_web_core::ProcedureDescriptor;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "new") {
        if args.len() != 6 || args[2] != "--name" || args[4] != "--framework-path" {
            return Err("Usage: loams-web-cli new DIRECTORY --name NAME --framework-path CHECKOUT_OR_FACADE_PATH".into());
        }
        let name = args[3]
            .to_str()
            .ok_or("Package name must be UTF-8")?
            .to_owned();
        let destination = scaffold(&ScaffoldOptions {
            directory: args[1].clone().into(),
            name,
            framework_path: args[5].clone().into(),
        })?;
        println!("Created {}", destination.display());
        return Ok(());
    }
    if args.len() != 3 || (args[1] != "--write" && args[1] != "--check") {
        return Err("Usage: loams-web-cli descriptors.json (--write|--check) client.ts".into());
    }
    let descriptors: Vec<ProcedureDescriptor> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let source = generate_typescript(&descriptors)?;
    if args[1] == "--write" {
        write_client(&args[2], &source)?;
    } else {
        check_client(&args[2], &source)?;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
