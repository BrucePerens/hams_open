//! `ham_proxy --listen ADDR --cert PEM --key PEM --site HOST:PORT [--ca PEM] [--verify HOST:PORT]
//! [--sign-key ED25519.pem] [--whitelist FILE | --signed-whitelist FILE --whitelist-key HEXFILE] [--gateway] [--ham-resolver IP:PORT] [--allow-private-upstream]
//! [--callsign CALL --log FILE]`: thread per connection.
use ham_proxy::login::{Challenges, HttpVerifier};
use ham_proxy::policy::{Policy, Whitelist};
use ham_proxy::{ham_acceptor, serve_one_policed, site_connector, ProxyError};
use openssl::pkey::{PKey, Private};
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

// [@ANCHOR: ham_proxy_load_key]
/// The whitelist public key: a file holding the 64 hex characters of the raw Ed25519 key.
fn load_key(path: &str) -> Result<Vec<u8>, ProxyError> {
    let text = std::fs::read_to_string(path)?;
    ham_proxy::signed_list::unhex(text.trim())
        .filter(|k| k.len() == 32)
        .ok_or_else(|| ProxyError::Tls(format!("{path}: not a 64 hex character Ed25519 key")))
}

// [@ANCHOR: ham_proxy_load_list]
/// The current signed list; an unreadable, unsigned, modified or expired file gives an empty whitelist (only `.ham` names).
fn load_list(file: &std::path::Path, key: &[u8]) -> Whitelist {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| ham_proxy::signed_list::verify_list(&t, key, now).ok())
        .unwrap_or_default()
}

// [@ANCHOR: ham_proxy_run]
fn run() -> Result<(), ProxyError> {
    let args: Vec<String> = std::env::args().collect();
    let need = |n: &str| arg(&args, n).ok_or_else(|| ProxyError::Tls(format!("missing {n}")));
    let listen = need("--listen")?;
    // `--gateway` serves any site the signed whitelist permits (from the request's Host); otherwise one fixed `--site`.
    let gateway = args.iter().any(|a| a == "--gateway");
    let site = if gateway {
        ham_proxy::GATEWAY.to_string()
    } else {
        need("--site")?
    };
    // `--ham-resolver IP:PORT`: where `.ham` names are looked up (AAAA), instead of the system resolver.
    if let Some(r) = arg(&args, "--ham-resolver") {
        ham_proxy::resolve::set_ham_resolver(&r);
    }
    if args.iter().any(|a| a == "--allow-private-upstream") {
        ham_proxy::resolve::allow_private_upstream();
    }
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
    // `--signed-whitelist FILE --whitelist-key HEXFILE` (the hams.com list, re-read and verified on every connection) or the plain
    // `--whitelist FILE`; either needs `--callsign` and `--log`.
    let signed = match (
        arg(&args, "--signed-whitelist"),
        arg(&args, "--whitelist-key"),
    ) {
        (Some(f), Some(k)) => Some((PathBuf::from(f), load_key(&k)?)),
        (None, None) => None,
        _ => {
            return Err(ProxyError::Tls(
                "--signed-whitelist needs --whitelist-key".to_string(),
            ))
        }
    };
    // `--sign-key PEM` (Ed25519 private key) turns on the signed `X-Ham-Integrity` header on HTML responses.
    let sign: Option<Arc<PKey<Private>>> = match arg(&args, "--sign-key") {
        Some(f) => Some(Arc::new(PKey::private_key_from_pem(&std::fs::read(f)?)?)),
        None => None,
    };
    let plain = arg(&args, "--whitelist");
    let policy = if signed.is_some() || plain.is_some() {
        let text = match &plain {
            Some(f) => std::fs::read_to_string(f)?,
            None => String::new(),
        };
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(need("--log")?)?;
        let call = need("--callsign")?;
        let p = Policy::new(Whitelist::parse(&text), &call, Box::new(log))
            .ok_or_else(|| ProxyError::Tls("empty --callsign".to_string()))?;
        Some(Arc::new(p))
    } else {
        None
    };
    if gateway && policy.is_none() {
        return Err(ProxyError::Tls("--gateway needs a whitelist".to_string()));
    }
    for conn in TcpListener::bind(&listen)?.incoming() {
        let Ok(tcp) = conn else { continue };
        let (a, c, s, n, l, p, k) = (
            acceptor.clone(),
            connector.clone(),
            site.clone(),
            name.clone(),
            login.clone(),
            policy.clone(),
            sign.clone(),
        );
        let sg = signed.clone();
        thread::spawn(move || {
            if let (Some((file, key)), Some(p)) = (sg.as_ref(), p.as_ref()) {
                p.replace(load_list(file, key));
            }
            let lg = l
                .as_ref()
                .map(|x| (&x.0, &x.1 as &dyn ham_proxy::login::Verifier));
            if let Err(e) = serve_one_policed(tcp, &a, &c, &s, &n, k.as_deref(), lg, p.as_deref()) {
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
