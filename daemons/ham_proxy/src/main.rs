//! `ham_proxy --listen ADDR --cert PEM --key PEM --site HOST:PORT [--ca PEM] [--verify HOST:PORT]
//! [--sign-key ED25519.pem] [--whitelist FILE | --signed-whitelist FILE --whitelist-key HEXFILE] [--gateway]
//! [--callsign CALL --log FILE]`: thread per connection. `--lan` instead runs the browser-facing listener (`run_lan`).
use ham_proxy::lan::{serve_lan, Ca, Upstream};
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

/// `--signed-whitelist FILE --whitelist-key HEXFILE` (the hams.com list, re-read and verified on every connection), if given.
fn load_signed(args: &[String]) -> Result<Option<(PathBuf, Vec<u8>)>, ProxyError> {
    match (
        arg(args, "--signed-whitelist"),
        arg(args, "--whitelist-key"),
    ) {
        (Some(f), Some(k)) => Ok(Some((PathBuf::from(f), load_key(&k)?))),
        (None, None) => Ok(None),
        _ => Err(ProxyError::Tls(
            "--signed-whitelist needs --whitelist-key".to_string(),
        )),
    }
}

/// The whitelist policy from `--whitelist FILE` or the signed list; either needs `--callsign` and `--log`.
fn load_policy(args: &[String], signed: bool) -> Result<Option<Arc<Policy>>, ProxyError> {
    let plain = arg(args, "--whitelist");
    if !signed && plain.is_none() {
        return Ok(None);
    }
    let need = |n: &str| arg(args, n).ok_or_else(|| ProxyError::Tls(format!("missing {n}")));
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
    Ok(Some(Arc::new(p)))
}

/// `--lan`: the browser-facing listener for the member's LAN (see `ham_proxy::lan`). Needs `--proxy-ca`, `--proxy-ca-key`, a whitelist,
/// `--callsign`, `--log`, `--resolver IP:PORT` (the node's `.ham` resolver) and, for whitelisted Internet sites, `--upstream-gateway HOST:PORT --upstream-ca PEM`.
fn run_lan(args: &[String]) -> Result<(), ProxyError> {
    let need = |n: &str| arg(args, n).ok_or_else(|| ProxyError::Tls(format!("missing {n}")));
    let ca = Arc::new(Ca::load(
        &PathBuf::from(need("--proxy-ca")?),
        &PathBuf::from(need("--proxy-ca-key")?),
    )?);
    let up = Arc::new(Upstream::gateway(
        &need("--upstream-gateway")?,
        &PathBuf::from(need("--upstream-ca")?),
    )?);
    let resolver = need("--resolver")?;
    let signed = load_signed(args)?;
    let policy = load_policy(args, signed.is_some())?
        .ok_or_else(|| ProxyError::Tls("--lan needs a whitelist".to_string()))?;
    let sign: Option<Arc<PKey<Private>>> = match arg(args, "--sign-key") {
        Some(f) => Some(Arc::new(PKey::private_key_from_pem(&std::fs::read(f)?)?)),
        None => None,
    };
    for conn in TcpListener::bind(need("--listen")?)?.incoming() {
        let Ok(tcp) = conn else { continue };
        let (ca, up, policy, resolver, sign, signed) = (
            ca.clone(),
            up.clone(),
            policy.clone(),
            resolver.clone(),
            sign.clone(),
            signed.clone(),
        );
        thread::spawn(move || {
            if let Some((file, key)) = signed.as_ref() {
                policy.replace(load_list(file, key));
            }
            if let Err(e) = serve_lan(tcp, &ca, &policy, &up, &resolver, sign.as_deref()) {
                eprintln!("ham_proxy: {e:?}");
            }
        });
    }
    Ok(())
}

// [@ANCHOR: ham_proxy_run]
fn run() -> Result<(), ProxyError> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--lan") {
        return run_lan(&args);
    }
    let need = |n: &str| arg(&args, n).ok_or_else(|| ProxyError::Tls(format!("missing {n}")));
    let listen = need("--listen")?;
    // `--gateway` serves any site the signed whitelist permits (from the request's Host); otherwise one fixed `--site`.
    let gateway = args.iter().any(|a| a == "--gateway");
    let site = if gateway {
        ham_proxy::GATEWAY.to_string()
    } else {
        need("--site")?
    };
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
    let signed = load_signed(&args)?;
    // `--sign-key PEM` (Ed25519 private key) turns on the signed `X-Ham-Integrity` header on HTML responses.
    let sign: Option<Arc<PKey<Private>>> = match arg(&args, "--sign-key") {
        Some(f) => Some(Arc::new(PKey::private_key_from_pem(&std::fs::read(f)?)?)),
        None => None,
    };
    let policy = load_policy(&args, signed.is_some())?;
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
