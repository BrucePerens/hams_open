#![allow(clippy::unwrap_used, clippy::expect_used)]
//! A stalled client cannot hold a connection thread forever, and the number of connections is capped.
use ham_proxy::lan::{serve_lan, Ca, Upstream};
use ham_proxy::policy::{Policy, Whitelist};
use ham_proxy::{set_idle_timeout, try_slot, MAX_CONNECTIONS};
use std::net::{TcpListener, TcpStream};

#[test]
// Tests [@ANCHOR: ham_proxy_set_idle_timeout]
// Tests [@ANCHOR: ham_proxy_limit_idle]
fn a_client_that_sends_nothing_is_dropped_after_the_idle_timeout() {
    set_idle_timeout(1);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let _silent = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (tcp, _) = l.accept().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ca = |args: &[&str]| {
        assert!(std::process::Command::new("openssl")
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    };
    let (c, k) = (dir.path().join("c.pem"), dir.path().join("k.pem"));
    ca(&[
        "req",
        "-x509",
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:prime256v1",
        "-nodes",
        "-subj",
        "/CN=t",
        "-days",
        "1",
        "-keyout",
        k.to_str().unwrap(),
        "-out",
        c.to_str().unwrap(),
    ]);
    let ca = Ca::load(&c, &k).unwrap();
    let up = Upstream::gateway("127.0.0.1:1", &c).unwrap();
    let policy = Policy::new(Whitelist::default(), "W1AW", Box::new(std::io::sink())).unwrap();
    let r = serve_lan(tcp, &ca, &policy, &up, "127.0.0.1:1", None);
    assert!(r.is_err(), "a silent client must end in an error"); // returning at all is the proof: without the idle timeout serve_lan blocks forever
}

#[test]
// Tests [@ANCHOR: ham_proxy_try_slot]
fn connection_slots_are_capped_and_given_back() {
    let held: Vec<_> = (0..MAX_CONNECTIONS).map(|_| try_slot().unwrap()).collect();
    assert!(try_slot().is_none());
    drop(held);
    assert!(try_slot().is_some());
}
