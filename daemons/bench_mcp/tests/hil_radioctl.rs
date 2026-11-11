// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware check for the owner's rig, ignored by default. Reads only: it asks `radioctld` for the rig's frequency, mode and signal
//! strength and prints them. It never sets anything and never keys the transmitter.
//!
//! Run, with `radioctld` already running for the rig you want to check (a `radioctld --config FILE` whose `[[rig]]` has `listen = "127.0.0.1:4540"`):
//! `RADIOCTL_HIL_PORT=4540 cargo test --test hil_radioctl -- --ignored --nocapture`

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

fn ask(s: &mut TcpStream, r: &mut BufReader<TcpStream>, cmd: &str) -> Vec<String> {
    s.write_all(format!("+\\{cmd}\n").as_bytes()).unwrap();
    let mut out = vec![];
    loop {
        let mut line = String::new();
        assert!(
            r.read_line(&mut line).unwrap() > 0,
            "radioctld closed the connection"
        );
        let line = line.trim().to_string();
        let done = line.starts_with("RPRT");
        out.push(line);
        if done {
            return out;
        }
    }
}

#[test]
#[ignore]
fn the_rig_answers_frequency_mode_and_signal_strength() {
    let port: u16 = std::env::var("RADIOCTL_HIL_PORT")
        .expect("set RADIOCTL_HIL_PORT to the port of a running radioctld for your rig")
        .parse()
        .unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    for cmd in ["get_freq", "get_mode", "get_level STRENGTH", "get_ptt"] {
        let reply = ask(&mut s, &mut r, cmd);
        println!("{cmd}: {reply:?}");
        assert!(reply.last().unwrap().starts_with("RPRT"), "{cmd}");
    }
}
