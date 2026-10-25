use std::{future::IntoFuture, net::SocketAddr};

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
    let (shutdown, shutdown_requested) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, bill_split::router())
        .with_graceful_shutdown(async {
            let _ = shutdown_requested.await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            let _ = shutdown.send(());
            // Stop accepting immediately and allow in-flight calls a bounded drain.
            if let Ok(result) = tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
                result?;
            }
        }
    }
    Ok(())
}
