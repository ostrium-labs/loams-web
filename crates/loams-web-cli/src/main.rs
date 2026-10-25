use loams_web_cli::{check_client, generate_typescript, write_client};
use loams_web_core::ProcedureDescriptor;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
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
