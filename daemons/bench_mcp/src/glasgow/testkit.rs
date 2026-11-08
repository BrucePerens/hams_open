// SPDX-License-Identifier: AGPL-3.0-or-later
//! A simulated Glasgow behind the `Runner` trait. It answers the command lines the module sends with the real output formats (the
//! voltage table was recorded from a revC3), keeps the port voltages and limits as the device does, writes capture files, and runs the
//! real helper script `ops.py` under a Python harness with fake interface objects (`testkit_harness.py`). Faults are switches.

use super::runner::{Out, Req, Runner};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const SERIAL: &str = "C3-20200101T000000Z";

#[derive(Default)]
pub struct State {
    /// port -> (supply, limit, trip)
    pub ports: BTreeMap<String, (f64, f64, f64)>,
    pub calls: Vec<Vec<String>>,
    /// Pins that were driven when the call ended; a park clears them.
    pub driven: bool,
    pub parks: u32,
    // faults
    pub lie_supply: Option<f64>,
    pub fail_safe: bool,
    pub fail_command: Option<String>,
    pub hang_applet: bool,
    pub garbage_voltage: bool,
    pub missing: bool,
    pub applet_exit: i32,
    pub applet_stderr: String,
    // scripted results
    pub spi_reply: String,
    pub jtag_log: String,
    pub vcd: String,
    pub uart_csv: String,
    pub i2c_devices: String,
    pub uart_reply: String,
    pub fail_pattern: bool,
}

pub struct Sim {
    pub st: Mutex<State>,
    pub tmp: PathBuf,
}

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/glasgow_fixtures/");

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

impl Sim {
    pub fn new(tmp: PathBuf) -> Sim {
        let mut st = State::default();
        st.ports.insert("A".into(), (0.0, 5.5, 0.1));
        st.ports.insert("B".into(), (0.0, 5.5, 0.1));
        st.vcd = fixture("capture.vcd");
        st.uart_csv = fixture("uart_analyzer.csv");
        st.jtag_log = fixture("jtag_scan_log.txt");
        st.spi_reply = "ffef4018".into();
        st.i2c_devices = r#"{"32":"a5","80":"0102030405"}"#.into();
        Sim { st: Mutex::new(st), tmp }
    }

    pub fn calls(&self) -> Vec<String> {
        self.st.lock().unwrap().calls.iter().map(|c| c.join(" ").replace(&format!("--serial {SERIAL} "), "")).collect()
    }

    fn table(st: &State) -> String {
        let mut s = String::from("Port\tVsupply\tVlimit\tIsupply\tIalert\tVsense\tVsense(alert)\n");
        for (p, (v, l, t)) in &st.ports {
            let shown = st.lie_supply.unwrap_or(*v);
            s.push_str(&format!("{p}\t{shown:.3}\t{l:.3}\t0.000\t{t:.3}\t{shown:.3}\t0.000-0.000\t\n"));
        }
        if st.garbage_voltage {
            return "Port\tVsupply\nA\tnot-a-number\n".into();
        }
        s
    }
}

fn ok(stdout: &str, stderr: &str) -> Result<Out, String> {
    Ok(Out { code: Some(0), stdout: stdout.into(), stderr: stderr.into(), ..Out::default() })
}

impl Runner for Sim {
    fn run(&self, req: &Req) -> Result<Out, String> {
        let mut a: Vec<String> = req.args.clone();
        let mut st = self.st.lock().unwrap();
        st.calls.push(a.clone());
        if st.missing {
            return Ok(Out { code: Some(1), stderr: "E: glasgow.hardware.device: missing permissions to open device 001/004\nE: glasgow.cli: device not found\n".into(), ..Out::default() });
        }
        if a.first().map(|s| s.as_str()) == Some("--serial") {
            assert_eq!(a[1], SERIAL, "the config's serial must be passed through");
            a.drain(..2);
        }
        let cmd = a.first().cloned().unwrap_or_default();
        if st.fail_command.as_deref() == Some(cmd.as_str()) {
            return Ok(Out { code: Some(1), stderr: format!("E: simulated failure of {cmd}\n"), ..Out::default() });
        }
        match cmd.as_str() {
            "list" => ok(&fixture("list.txt"), &fixture("list_stderr.txt")),
            "safe" => {
                if st.fail_safe {
                    return Ok(Out { code: Some(1), stderr: "E: cannot talk to the device\n".into(), ..Out::default() });
                }
                for v in st.ports.values_mut() {
                    v.0 = 0.0;
                }
                st.lie_supply = None;
                st.driven = false;
                ok("", "I: glasgow.cli: all ports safe\n")
            }
            "voltage" => {
                if a.len() >= 3 {
                    let v: f64 = a[2].parse().unwrap();
                    let limit = st.ports[&a[1]].1;
                    assert!(v <= limit + 1e-9, "the device refuses a voltage above its limit: {v} > {limit}");
                    let e = st.ports.get_mut(&a[1]).unwrap();
                    e.0 = v;
                    if let Some(t) = a.get(3) {
                        e.2 = t.parse().unwrap();
                    }
                }
                ok(&Sim::table(&st), "")
            }
            "voltage-limit" => {
                if a.len() >= 3 {
                    st.ports.get_mut(&a[1]).unwrap().1 = a[2].parse().unwrap();
                }
                ok(&Sim::table(&st), "")
            }
            "build" => {
                let f = &a[a.iter().position(|x| x == "-f").unwrap() + 1];
                std::fs::write(f, b"bitstream").unwrap();
                let rev = &a[a.iter().position(|x| x == "--rev").unwrap() + 1];
                ok("", &format!("I: glasgow.cli: generating bitstream for applet {:?} (rev {rev})\n", a.last().unwrap()))
            }
            "run" | "script" => {
                if st.hang_applet && a[1] != "control-gpio" {
                    return Ok(Out { timed_out: true, stderr: "I: running\n".into(), ..Out::default() });
                }
                if st.applet_exit != 0 && a[1] != "control-gpio" {
                    return Ok(Out { code: Some(st.applet_exit), stderr: st.applet_stderr.clone(), ..Out::default() });
                }
                let applet = if cmd == "run" { a[1].clone() } else { a[2].clone() };
                match (cmd.as_str(), applet.as_str()) {
                    ("run", "control-gpio") => {
                        st.parks += 1;
                        st.driven = a.iter().any(|x| x.ends_with("=1") || x.ends_with("=0"));
                        let reads: String = a.iter().filter(|x| x.len() == 2 && x.starts_with(['A', 'B'])).map(|x| format!("{x}=1\n")).collect();
                        ok(&reads, "I: glasgow.cli: running handler for applet 'control-gpio'\n")
                    }
                    ("run", "spi-controller") => ok(&format!("{}\n", st.spi_reply), ""),
                    ("run", "jtag-probe") => ok("", &st.jtag_log.clone()),
                    ("run", "analyzer") => {
                        std::fs::write(a.last().unwrap(), &st.vcd).unwrap();
                        Ok(Out { code: Some(0), stopped: req.stop_after.is_some(), ..Out::default() })
                    }
                    ("run", "uart-analyzer") => {
                        std::fs::write(a.last().unwrap(), &st.uart_csv).unwrap();
                        Ok(Out { code: Some(0), stopped: req.stop_after.is_some(), ..Out::default() })
                    }
                    ("script", _) => {
                        let released = self.tmp.join("released.txt");
                        let harness = self.tmp.join("harness.py");
                        std::fs::write(&harness, include_str!("testkit_harness.py")).unwrap();
                        let dd = a.iter().position(|x| x == "--").unwrap();
                        let cfg = serde_json::json!({"i2c_devices": serde_json::from_str::<serde_json::Value>(&st.i2c_devices).unwrap(), "uart_reply": st.uart_reply, "gpio_count": 4, "fail_on_set": st.fail_pattern});
                        let out = std::process::Command::new("python3")
                            .arg("-I")
                            .arg(&harness)
                            .arg(&a[1])
                            .arg(&a[2])
                            .arg(&a[dd + 1])
                            .arg(&a[dd + 2])
                            .env("SIM_CFG", cfg.to_string())
                            .env("SIM_RELEASED", &released)
                            .output()
                            .map_err(|e| e.to_string())?;
                        st.driven = false;
                        Ok(Out { code: out.status.code(), stdout: String::from_utf8_lossy(&out.stdout).into(), stderr: String::from_utf8_lossy(&out.stderr).into(), ..Out::default() })
                    }
                    (c, ap) => Err(format!("the simulator has no answer for {c} {ap}: {a:?}")),
                }
            }
            other => Err(format!("the simulator has no answer for {other:?}: {a:?}")),
        }
    }
}
