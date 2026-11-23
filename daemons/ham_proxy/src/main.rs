//! `ham_proxy --listen ADDR --cert PEM --key PEM --site HOST:PORT [--ca PEM]`: thread per connection.
use ham_proxy::login::{Challenges, HttpVerifier};
use ham_proxy::{ham_acceptor, serve_one_login, site_connector, ProxyError};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

// [@ANCHOR: ham_proxy_arg]
fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

// [@ANCHOR: ham_proxy_run]
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
    // `--verify HOST:PORT` turns on `/.ham/login`; the host name is also the certificate name checked.
    let login = match arg(&args, "--verify") {
        Some(addr) => {
            let host = addr
                .rsplit_once(':')
                .map_or(addr.as_str(), |(h, _)| h)
                .to_string();
            Some(Arc::new((
                Challenges::default(),
                HttpVerifier {
                    connector: site_connector(ca.as_deref())?,
                    addr,
                    host,
                },
            )))
        }
        None => None,
    };
    for conn in TcpListener::bind(&listen)?.incoming() {
        let Ok(tcp) = conn else { continue };
        let (a, c, s, n, l) = (
            acceptor.clone(),
            connector.clone(),
            site.clone(),
            name.clone(),
            login.clone(),
        );
        thread::spawn(move || {
            let lg = l
                .as_ref()
                .map(|x| (&x.0, &x.1 as &dyn ham_proxy::login::Verifier));
            if let Err(e) = serve_one_login(tcp, &a, &c, &s, &n, None, lg) {
                eprintln!("ham_proxy: {e:?}");
            }
        });
    }
    Ok(())
}

// [@ANCHOR: ham_proxy_main]
fn main() {
    if let Err(e) = run() {
        eprintln!("ham_proxy: {e:?}");
        std::process::exit(1);
    }
}
