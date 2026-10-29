// SPDX-License-Identifier: AGPL-3.0-or-later
//! `vnwa_open install-system-config`: puts the files that keep the sound server away from the VNWA's USB codecs under `/etc` (the WirePlumber rule, the udev
//! rule and its helper script, all from `packaging/`, with their comment blocks intact). Cautions, all enforced here and tested:
//! * dry run unless `--apply`; the plan lists every file with its full content and every command;
//! * idempotent (a file that already has the packaged content is left alone), reversible (a replaced file is first copied to `<name>.vnwa-backup`;
//!   `--uninstall` puts it back or removes ours), and `--check` changes nothing;
//! * the only commands it can run are in `allowed`: `install`, `cp`, `mv`, `rm` on the named files and `udevadm control --reload`. Never
//!   `udevadm trigger`, never a restart or reload of the audio service (WirePlumber has no reload and a restart drops every client of the desktop);
//!   `--restart-audio` is a separate opt-in that adds exactly one command, `systemctl --user restart wireplumber`;
//! * refuses while a sweep or soak has a VNWA codec open, unless `--force`;
//! * a failure is a clear message and the steps done so far are undone; every action is appended to a log file;
//! * nothing outside the three named files is touched, no network, no packages.

use std::path::{Path, PathBuf};

pub const WIREPLUMBER_RULE: &str = include_str!("../packaging/wireplumber/51-vnwa-codecs.conf");
pub const UDEV_RULE: &str = include_str!("../packaging/udev/60-vnwa-codecs.rules");
pub const HELPER: &str = include_str!("../packaging/udev/vnwa-is-codec");

pub const WIREPLUMBER_DEST: &str = "/etc/wireplumber/wireplumber.conf.d/51-vnwa-codecs.conf";
pub const UDEV_DEST: &str = "/etc/udev/rules.d/60-vnwa-codecs.rules";
pub const HELPER_DEST: &str = "/usr/local/libexec/vnwa-is-codec";
pub const BACKUP_SUFFIX: &str = ".vnwa-backup";

/// One file we manage.
#[derive(Clone, Debug)]
pub struct Managed {
    pub dest: &'static str,
    pub content: &'static str,
    pub mode: &'static str,
}

pub fn managed() -> Vec<Managed> {
    vec![
        Managed {
            dest: WIREPLUMBER_DEST,
            content: WIREPLUMBER_RULE,
            mode: "0644",
        },
        Managed {
            dest: UDEV_DEST,
            content: UDEV_RULE,
            mode: "0644",
        },
        Managed {
            dest: HELPER_DEST,
            content: HELPER,
            mode: "0755",
        },
    ]
}

/// What the installer was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Install,
    Uninstall,
    Check,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    pub apply: bool,
    pub force: bool,
    pub restart_audio: bool,
    /// A directory the destinations are placed under instead of `/` (like DESTDIR); no `sudo`, no `udevadm`.
    pub root: Option<PathBuf>,
}

/// The state of one managed file.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Missing,
    Current,
    Different,
}

/// Runs a command; production is a process, tests record.
pub trait Exec {
    fn run(&self, argv: &[String]) -> Result<(), String>;
}

pub struct ProcessExec;

impl Exec for ProcessExec {
    fn run(&self, argv: &[String]) -> Result<(), String> {
        let (prog, args) = argv.split_first().ok_or("empty command")?;
        let out = std::process::Command::new(prog)
            .args(args)
            .output()
            .map_err(|e| format!("{}: {e}", argv.join(" ")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{} failed: {}",
                argv.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

/// True if `argv` is one of the commands the installer may run. Everything else is refused before it is run.
pub fn allowed(argv: &[String], restart_audio: bool) -> bool {
    // [@ANCHOR: vnwa_open_install_commands]
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    let a = if a.first() == Some(&"sudo") {
        &a[1..]
    } else {
        &a[..]
    };
    let a: Vec<&str> = if a.first() == Some(&"-n") {
        a[1..].to_vec()
    } else {
        a.to_vec()
    };
    let ours = |p: &str| {
        managed().iter().any(|m| {
            p == m.dest
                || p == format!("{}{BACKUP_SUFFIX}", m.dest)
                || p.ends_with(m.dest)
                || p.ends_with(&format!("{}{BACKUP_SUFFIX}", m.dest))
        })
    };
    match a.as_slice() {
        ["udevadm", "control", "--reload"] => true,
        ["systemctl", "--user", "restart", "wireplumber"] => restart_audio,
        ["install", "-D", "-m", _, src, dest] => ours(dest) && !src.is_empty(),
        ["cp", "-p", src, dest] => ours(src) && ours(dest),
        ["mv", src, dest] => ours(src) && ours(dest),
        ["rm", "-f", p] => ours(p),
        _ => false,
    }
}

/// The installer: options, an executor and the sysfs / proc locations.
pub struct Installer<'a> {
    pub opts: Options,
    pub exec: &'a dyn Exec,
    pub sys: PathBuf,
    pub proc_asound: PathBuf,
    pub log: PathBuf,
    pub root_user: bool,
}

impl Installer<'_> {
    fn path(&self, dest: &str) -> PathBuf {
        match &self.opts.root {
            Some(r) => r.join(dest.trim_start_matches('/')),
            None => PathBuf::from(dest),
        }
    }

    pub fn state(&self, m: &Managed) -> State {
        match std::fs::read_to_string(self.path(m.dest)) {
            Err(_) => State::Missing,
            Ok(s) if s == m.content => State::Current,
            Ok(_) => State::Different,
        }
    }

    fn command(&self, mut argv: Vec<String>) -> Vec<String> {
        if self.opts.root.is_none() && !self.root_user {
            argv.insert(0, "-n".into());
            argv.insert(0, "sudo".into());
        }
        argv
    }

    fn do_run(&self, argv: Vec<String>, done: &mut Vec<String>) -> Result<(), String> {
        let argv = self.command(argv);
        if !allowed(&argv, self.opts.restart_audio) {
            return Err(format!(
                "refused to run a command that is not one of the installer's: {}",
                argv.join(" ")
            ));
        }
        self.exec.run(&argv)?;
        self.note(&argv.join(" "));
        done.push(argv.join(" "));
        Ok(())
    }

    fn note(&self, line: &str) {
        use std::io::Write;
        if let Some(d) = self.log.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
        {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "{t} {line}");
        }
    }

    /// A codec that a sweep or soak holds open (its capture stream is not closed).
    pub fn busy(&self) -> Vec<String> {
        let mut busy = Vec::new();
        for c in crate::topology::find_cards(&self.sys) {
            if !crate::reserved::is_vnwa_codec(&self.sys, &c.usb_path) {
                continue;
            }
            let Ok(dir) = std::fs::read_dir(&self.proc_asound) else {
                continue;
            };
            for e in dir.flatten() {
                let id = std::fs::read_to_string(e.path().join("id")).unwrap_or_default();
                if id.trim() != c.card_id {
                    continue;
                }
                if let Ok(s) = std::fs::read_to_string(e.path().join("pcm0c/sub0/status")) {
                    if s.trim() != "closed" {
                        busy.push(format!(
                            "{} ({})",
                            c.card_id,
                            s.lines().next().unwrap_or("").trim()
                        ));
                    }
                }
            }
        }
        busy
    }

    /// The plan as text: for each file its action, and the commands. In a dry run this is all that happens.
    pub fn plan(&self) -> String {
        let mut out = String::new();
        let restore = self.opts.mode == Mode::Uninstall;
        for m in managed() {
            let st = self.state(&m);
            let dest = self.path(m.dest);
            let action = match (&self.opts.mode, &st) {
                (Mode::Check, _) => format!("{:?}", st),
                (Mode::Install, State::Current) => {
                    "unchanged (already the packaged content)".to_string()
                }
                (Mode::Install, State::Missing) => "create".to_string(),
                (Mode::Install, State::Different) => format!(
                    "replace (the old file is first copied to {}{BACKUP_SUFFIX})",
                    dest.display()
                ),
                (Mode::Uninstall, State::Missing) => "nothing to remove".to_string(),
                (Mode::Uninstall, _) => {
                    if self.backup_exists(&m) {
                        format!("restore {}{BACKUP_SUFFIX}", dest.display())
                    } else if st == State::Current || self.opts.force {
                        "remove".to_string()
                    } else {
                        "leave (it differs from ours and there is no backup: use --force)"
                            .to_string()
                    }
                }
            };
            out.push_str(&format!(
                "{} {} (mode {}): {action}\n",
                if restore { "-" } else { "*" },
                dest.display(),
                m.mode
            ));
            if self.opts.mode == Mode::Install && st != State::Current {
                for l in m.content.lines() {
                    out.push_str(&format!("    | {l}\n"));
                }
            }
        }
        if self.opts.mode != Mode::Check && self.opts.root.is_none() {
            out.push_str("then: udevadm control --reload (only if a udev file changed; it re-announces nothing)\n");
        }
        if self.opts.restart_audio && self.opts.mode != Mode::Check {
            out.push_str("then, because --restart-audio was given: systemctl --user restart wireplumber (WARNING: briefly drops every audio client, the desktop shell included)\n");
        }
        if self.opts.mode == Mode::Install {
            out.push_str("The rule takes effect at the next login or boot, or after the unit is replugged. Nothing is restarted or re-announced.\n");
        }
        out
    }

    fn backup_exists(&self, m: &Managed) -> bool {
        PathBuf::from(format!("{}{BACKUP_SUFFIX}", self.path(m.dest).display())).exists()
    }

    /// Does what was asked. Without `apply` it only returns the plan.
    pub fn run(&self) -> Result<String, String> {
        let mut text = self.plan();
        if self.opts.mode == Mode::Check || !self.opts.apply {
            if self.opts.mode != Mode::Check {
                text.push_str("DRY RUN: nothing was changed. Add --apply to do it.\n");
            }
            return Ok(text);
        }
        if !self.opts.force {
            let busy = self.busy();
            if !busy.is_empty() {
                return Err(format!("a measurement holds the VNWA codec(s) open ({}): not changing system files now; wait for it to finish or use --force", busy.join(", ")));
            }
        }
        let mut done: Vec<(Managed, Undo)> = Vec::new();
        let result = match self.opts.mode {
            Mode::Install => self.install(&mut done),
            Mode::Uninstall => self.uninstall(),
            Mode::Check => Ok(()),
        };
        if let Err(e) = result {
            self.note(&format!("FAILED: {e}; undoing {} step(s)", done.len()));
            for (m, undo) in done.into_iter().rev() {
                let mut sink = Vec::new();
                let argv = match undo {
                    Undo::Remove => vec![
                        "rm".to_string(),
                        "-f".into(),
                        self.path(m.dest).display().to_string(),
                    ],
                    Undo::Restore => vec![
                        "mv".to_string(),
                        format!("{}{BACKUP_SUFFIX}", self.path(m.dest).display()),
                        self.path(m.dest).display().to_string(),
                    ],
                };
                let _ = self.do_run(argv, &mut sink);
            }
            return Err(format!("{e} (the steps done before it were undone)"));
        }
        text.push_str("Done.\n");
        Ok(text)
    }

    fn install(&self, done: &mut Vec<(Managed, Undo)>) -> Result<(), String> {
        let mut udev_changed = false;
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let tmp = std::env::temp_dir().join(format!(
            "vnwa_open_install_{}_{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let result = (|| -> Result<(), String> {
            for m in managed() {
                let st = self.state(&m);
                if st == State::Current {
                    continue;
                }
                let dest = self.path(m.dest);
                let mut sink = Vec::new();
                let undo = if st == State::Different {
                    let backup = format!("{}{BACKUP_SUFFIX}", dest.display());
                    if !PathBuf::from(&backup).exists() {
                        self.do_run(
                            vec!["cp".into(), "-p".into(), dest.display().to_string(), backup],
                            &mut sink,
                        )?;
                    }
                    Undo::Restore
                } else {
                    Undo::Remove
                };
                let src = tmp.join(Path::new(m.dest).file_name().unwrap());
                std::fs::write(&src, m.content).map_err(|e| format!("{}: {e}", src.display()))?;
                done.push((m.clone(), undo));
                self.do_run(
                    vec![
                        "install".into(),
                        "-D".into(),
                        "-m".into(),
                        m.mode.into(),
                        src.display().to_string(),
                        dest.display().to_string(),
                    ],
                    &mut sink,
                )?;
                if m.dest == UDEV_DEST || m.dest == HELPER_DEST {
                    udev_changed = true;
                }
            }
            if udev_changed && self.opts.root.is_none() {
                self.do_run(
                    vec!["udevadm".into(), "control".into(), "--reload".into()],
                    &mut Vec::new(),
                )?;
            }
            if self.opts.restart_audio {
                self.do_run(
                    vec![
                        "systemctl".into(),
                        "--user".into(),
                        "restart".into(),
                        "wireplumber".into(),
                    ],
                    &mut Vec::new(),
                )?;
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        result
    }

    fn uninstall(&self) -> Result<(), String> {
        let mut udev_changed = false;
        for m in managed() {
            let st = self.state(&m);
            let dest = self.path(m.dest).display().to_string();
            let backup = format!("{dest}{BACKUP_SUFFIX}");
            let mut sink = Vec::new();
            if PathBuf::from(&backup).exists() {
                self.do_run(vec!["mv".into(), backup, dest], &mut sink)?;
            } else if st == State::Current || (st == State::Different && self.opts.force) {
                self.do_run(vec!["rm".into(), "-f".into(), dest], &mut sink)?;
            } else {
                continue;
            }
            if m.dest == UDEV_DEST || m.dest == HELPER_DEST {
                udev_changed = true;
            }
        }
        if udev_changed && self.opts.root.is_none() {
            self.do_run(
                vec!["udevadm".into(), "control".into(), "--reload".into()],
                &mut Vec::new(),
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Undo {
    Remove,
    Restore,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reserved::fixtures::tmp;
    use std::sync::Mutex;

    /// Runs each command for real under the staging root (`install`, `cp`, `mv`, `rm` are plain file operations) and records it.
    struct Recording(Mutex<Vec<Vec<String>>>);
    impl Exec for Recording {
        fn run(&self, argv: &[String]) -> Result<(), String> {
            self.0.lock().unwrap().push(argv.to_vec());
            ProcessExec.run(argv)
        }
    }

    fn installer<'a>(root: &Path, exec: &'a dyn Exec, mode: Mode, apply: bool) -> Installer<'a> {
        Installer {
            opts: Options {
                mode,
                apply,
                force: false,
                restart_audio: false,
                root: Some(root.to_path_buf()),
            },
            exec,
            sys: root.join("sys"),
            proc_asound: root.join("proc/asound"),
            log: root.join("var/log/install.log"),
            root_user: false,
        }
    }

    // Tests [@ANCHOR: vnwa_open_install_commands]
    #[test]
    fn a_dry_run_prints_the_files_and_changes_nothing_and_every_packaged_file_keeps_its_comment_block(
    ) {
        let root = tmp("inst_dry");
        let rec = Recording(Mutex::new(Vec::new()));
        let text = installer(&root, &rec, Mode::Install, false).run().unwrap();
        assert!(
            text.contains("DRY RUN")
                && text.contains("create")
                && text.contains("| monitor.alsa.rules"),
            "{text}"
        );
        assert!(rec.0.lock().unwrap().is_empty());
        assert!(!root.join("etc").exists());
        for m in managed() {
            for needle in [
                "ADDED BY:",
                "https://github.com/BrucePerens/hams_open",
                "WHAT:",
                "TO REMOVE:",
            ] {
                assert!(m.content.contains(needle), "{} lacks {needle}", m.dest);
            }
        }
        for m in [WIREPLUMBER_RULE, UDEV_RULE] {
            for needle in ["WHY:", "SCOPE:", "CAVEAT:"] {
                assert!(m.contains(needle), "{needle}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_installs_is_idempotent_backs_up_what_it_replaces_and_uninstall_restores_it() {
        let root = tmp("inst_apply");
        let rec = Recording(Mutex::new(Vec::new()));
        // An older file of someone else at the udev destination.
        let udev = root.join(UDEV_DEST.trim_start_matches('/'));
        std::fs::create_dir_all(udev.parent().unwrap()).unwrap();
        std::fs::write(&udev, "# somebody's own rule\n").unwrap();
        installer(&root, &rec, Mode::Install, true).run().unwrap();
        for m in managed() {
            assert_eq!(
                std::fs::read_to_string(root.join(m.dest.trim_start_matches('/'))).unwrap(),
                m.content
            );
        }
        assert_eq!(
            std::fs::read_to_string(format!("{}{BACKUP_SUFFIX}", udev.display())).unwrap(),
            "# somebody's own rule\n"
        );
        let n = rec.0.lock().unwrap().len();
        // Second run: nothing to do.
        let again = installer(&root, &rec, Mode::Install, true).run().unwrap();
        assert!(again.contains("unchanged"), "{again}");
        assert_eq!(
            rec.0.lock().unwrap().len(),
            n,
            "idempotent: no command the second time"
        );
        // Check reports and changes nothing.
        let chk = installer(&root, &rec, Mode::Check, false).run().unwrap();
        assert!(chk.contains("Current") && !chk.contains("DRY RUN"), "{chk}");
        assert_eq!(rec.0.lock().unwrap().len(), n);
        // Uninstall: the udev file is restored from its backup, the others removed.
        installer(&root, &rec, Mode::Uninstall, true).run().unwrap();
        assert_eq!(
            std::fs::read_to_string(&udev).unwrap(),
            "# somebody's own rule\n"
        );
        assert!(!root.join(WIREPLUMBER_DEST.trim_start_matches('/')).exists());
        assert!(!root.join(HELPER_DEST.trim_start_matches('/')).exists());
        // The log says what was done.
        let log = std::fs::read_to_string(root.join("var/log/install.log")).unwrap();
        assert!(log.contains("install -D") && log.contains("mv "), "{log}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_command_outside_the_allow_list_is_ever_issued_and_trigger_restart_and_reload_are_refused()
    {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(allowed(
            &s(&["sudo", "-n", "udevadm", "control", "--reload"]),
            false
        ));
        assert!(allowed(
            &s(&["sudo", "-n", "install", "-D", "-m", "0644", "/tmp/x", UDEV_DEST]),
            false
        ));
        assert!(allowed(&s(&["rm", "-f", WIREPLUMBER_DEST]), false));
        for bad in [
            &["udevadm", "trigger"][..],
            &[
                "udevadm",
                "trigger",
                "--action=add",
                "/sys/class/sound/card1",
            ],
            &["udevadm", "control", "--reload-rules"],
            &["udevadm", "settle"],
            &["systemctl", "--user", "restart", "wireplumber"],
            &["systemctl", "--user", "reload", "wireplumber"],
            &["pkill", "-HUP", "wireplumber"],
            &["wpctl", "set-profile", "1", "0"],
            &["rm", "-f", "/etc/passwd"],
            &["rm", "-rf", "/etc/udev"],
            &["install", "-D", "-m", "0644", "/tmp/x", "/etc/passwd"],
            &["mv", "/etc/passwd", UDEV_DEST],
            &["curl", "http://example.invalid"],
            &["apt-get", "install", "x"],
        ] {
            assert!(!allowed(&s(bad), false), "{bad:?} must be refused");
        }
        // Only the explicit opt-in allows the one restart command.
        assert!(allowed(
            &s(&["systemctl", "--user", "restart", "wireplumber"]),
            true
        ));
        assert!(!allowed(
            &s(&["systemctl", "--user", "restart", "pipewire"]),
            true
        ));
    }

    #[test]
    fn a_failure_midway_is_a_clear_error_and_the_steps_done_are_undone() {
        let root = tmp("inst_fail");
        struct FailsOnThird(Mutex<u32>);
        impl Exec for FailsOnThird {
            fn run(&self, argv: &[String]) -> Result<(), String> {
                let mut n = self.0.lock().unwrap();
                *n += 1;
                if *n == 3 && argv[0] == "install" {
                    return Err("disk full".into());
                }
                ProcessExec.run(argv)
            }
        }
        let exec = FailsOnThird(Mutex::new(0));
        let e = installer(&root, &exec, Mode::Install, true)
            .run()
            .unwrap_err();
        assert!(e.contains("disk full") && e.contains("undone"), "{e}");
        for m in managed() {
            assert!(
                !root.join(m.dest.trim_start_matches('/')).exists(),
                "{} was left behind",
                m.dest
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn it_refuses_while_a_codec_is_open_unless_forced() {
        let root = tmp("inst_busy");
        crate::reserved::fixtures::tree(
            &root.join("sys"),
            &[("3-2.2.1", "20a0", "4118"), ("3-2.2.2", "08bb", "29c0")],
        );
        let card = root.join("sys/class/sound/card1");
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(card.join("id"), "CODEC\n").unwrap();
        let iface = root.join("sys/bus/usb/devices/3-2.2.2/3-2.2.2:1.0");
        std::fs::create_dir_all(&iface).unwrap();
        std::os::unix::fs::symlink(&iface, card.join("device")).unwrap();
        let pcm = root.join("proc/asound/card1/pcm0c/sub0");
        std::fs::create_dir_all(&pcm).unwrap();
        std::fs::write(root.join("proc/asound/card1/id"), "CODEC\n").unwrap();
        std::fs::write(pcm.join("status"), "state: RUNNING\nowner_pid   : 123\n").unwrap();
        let rec = Recording(Mutex::new(Vec::new()));
        let e = installer(&root, &rec, Mode::Install, true)
            .run()
            .unwrap_err();
        assert!(
            e.contains("holds the VNWA codec") && e.contains("CODEC"),
            "{e}"
        );
        assert!(rec.0.lock().unwrap().is_empty());
        std::fs::write(pcm.join("status"), "closed\n").unwrap();
        installer(&root, &rec, Mode::Install, true).run().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
