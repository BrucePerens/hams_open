//! `ham_proxy --listen ADDR --cert PEM --key PEM --site HOST:PORT [--ca PEM]`: thread per connection.
use ham_proxy::{ham_acceptor, serve_one, site_connector, ProxyError};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn run() -> Result<(), ProxyError> {
    let args: Vec<String> = std::env::args().collect();
    let need = |n: &str| arg(&args, n).ok_or_else(|| ProxyError::Tls(format!("missing {n}")));
    let listen = need("--listen")?;
    let site = need("--site")?;
    let acceptor = Arc::new(ham_acceptor(
        &PathBuf::from(need("--cert")?),
        &PathBuf::from(need("--key")?),
    )?);
    let ca = arg(&args, "--ca").map(PathBuf::from);
    let connector = Arc::new(site_connector(ca.as_deref())?);
    let name = site
        .rsplit_once(':')
        .map_or(site.as_str(), |(h, _)| h)
        .to_string();
    for conn in TcpListener::bind(&listen)?.incoming() {
        let Ok(tcp) = conn else { continue };
        let (a, c, s, n) = (
            acceptor.clone(),
            connector.clone(),
            site.clone(),
            name.clone(),
        );
        thread::spawn(move || {
            if let Err(e) = serve_one(tcp, &a, &c, &s, &n) {
                eprintln!("ham_proxy: {e:?}");
            }
        });
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ham_proxy: {e:?}");
        std::process::exit(1);
    }
}
