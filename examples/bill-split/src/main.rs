use std::net::SocketAddr;

use loams_web::http::server::{Server, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--openapi") if args.next().is_none() => {
            println!("{}", serde_json::to_string_pretty(&bill_split::openapi())?);
            return Ok(());
        }
        Some("--client") if args.next().is_none() => {
            print!(
                "{}",
                loams_web_cli::generate_typescript(&[bill_split::split().descriptor().clone()])?
            );
            return Ok(());
        }
        Some("--descriptors") if args.next().is_none() => {
            println!(
                "{}",
                serde_json::to_string_pretty(&[bill_split::split().descriptor()])?
            );
            return Ok(());
        }
        Some(_) => return Err("usage: bill-split [--openapi | --client | --descriptors]".into()),
        None => {}
    }
    let address: SocketAddr = std::env::var("LOAMS_WEB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!("Bill split listening on http://{}", listener.local_addr()?);
    Server::new(ServerConfig::default())?
        .serve(listener, bill_split::router(), async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
