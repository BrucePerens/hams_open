//! `bench ADDR CA PATH CONNS THREADS [tls12]`: integrity-only HTTPS GETs against ADDR (the proxy under test),
//! one connection per request. Prints requests/s and MB/s. Same client for every proxy being compared.
use ham_proxy::ham_connector;
use openssl::ssl::{SslConnector, SslMethod, SslVersion};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        return Err("usage: bench ADDR CA PATH CONNS THREADS [tls12]".into());
    }
    let (addr, ca, path) = (a[1].clone(), a[2].clone(), a[3].clone());
    let (conns, threads): (usize, usize) = (a[4].parse()?, a[5].parse()?);
    let conn = if a.get(6).map(String::as_str) == Some("tls12") {
        let mut b = SslConnector::builder(SslMethod::tls_client())?;
        b.set_max_proto_version(Some(SslVersion::TLS1_2))?;
        b.set_cipher_list("NULL-SHA256:@SECLEVEL=0")?;
        b.set_ca_file(Path::new(&ca))?;
        b.build()
    } else {
        ham_connector(Path::new(&ca))?
    };
    let conn = Arc::new(conn);
    let t0 = Instant::now();
    let hs: Vec<_> = (0..threads)
        .map(|_| {
            let (conn, addr, path) = (conn.clone(), addr.clone(), path.clone());
            thread::spawn(move || -> Result<(usize, usize), String> {
                let (mut ok, mut bytes) = (0, 0);
                for _ in 0..conns / threads {
                    let tcp = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
                    let mut s = conn
                        .connect("localhost", tcp)
                        .map_err(|e| format!("{e:?}"))?;
                    s.write_all(
                        format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes(),
                    )
                    .map_err(|e| e.to_string())?;
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).ok();
                    if buf.starts_with(b"HTTP/1.") {
                        ok += 1;
                        bytes += buf.len();
                    }
                }
                Ok((ok, bytes))
            })
        })
        .collect();
    let (mut ok, mut bytes) = (0, 0);
    for h in hs {
        let (o, b) = h.join().map_err(|_| "thread panicked")??;
        ok += o;
        bytes += b;
    }
    let secs = t0.elapsed().as_secs_f64();
    println!(
        "ok={ok} secs={secs:.2} req/s={:.0} MB/s={:.1}",
        ok as f64 / secs,
        bytes as f64 / secs / 1e6
    );
    Ok(())
}
