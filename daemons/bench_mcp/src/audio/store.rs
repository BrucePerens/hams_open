// SPDX-License-Identifier: AGPL-3.0-or-later
//! The managed capture directory: where recordings and generated tones go, with a retention limit. Recordings may contain
//! speech, so they stay on this machine and no tool uploads them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `YYYYMMDD-HHMMSS` in local time.
pub fn timestamp() -> String {
    let mut t: libc::time_t = 0;
    // SAFETY: time and localtime_r write only to the locals given.
    let tm = unsafe {
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// A file name part made of letters, digits and dashes only.
pub fn slug(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    t.trim_matches('-').chars().take(40).collect()
}

/// Creates the directory (owner only) and returns a fresh path `<prefix>-<timestamp>[-<label>].wav` in it.
pub fn new_path(dir: &Path, prefix: &str, label: Option<&str>) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let label = label.map(slug).filter(|l| !l.is_empty());
    let base = match label {
        Some(l) => format!("{prefix}-{}-{l}", timestamp()),
        None => format!("{prefix}-{}", timestamp()),
    };
    let mut p = dir.join(format!("{base}.wav"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{base}-{n}.wav"));
        n += 1;
    }
    Ok(p)
}

/// A file named by the caller, which must be a `.wav` file inside the managed directory.
pub fn managed_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    // [@ANCHOR: audio_store_managed]
    let p = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else {
        dir.join(name)
    };
    let real = std::fs::canonicalize(&p).map_err(|e| format!("{name}: {e}"))?;
    let root = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !real.starts_with(&root) {
        return Err(format!(
            "{name} is outside the managed audio directory {}",
            root.display()
        ));
    }
    if real.extension().and_then(|e| e.to_str()) != Some("wav") {
        return Err(format!("{name} is not a .wav file"));
    }
    Ok(real)
}

/// Deletes managed `.wav` files (except `ref-` reference recordings) older than `days`, then the oldest ones beyond `max_files`. Returns the number removed.
pub fn prune(dir: &Path, max_files: usize, days: u64) -> usize {
    // [@ANCHOR: audio_store_retention]
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
        // Reference recordings are kept until someone deletes them.
        .filter(|e| !e.file_name().to_string_lossy().starts_with("ref-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(days * 86400))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    let total = files.len();
    for (i, (when, path)) in files.iter().enumerate() {
        let too_old = *when < cutoff;
        let too_many = total - removed > max_files && i < total;
        if (too_old || too_many) && std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("audio-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Tests [@ANCHOR: audio_store_managed]
    #[test]
    fn only_wav_files_inside_the_directory_are_accepted() {
        let d = scratch("managed");
        std::fs::write(d.join("a.wav"), b"x").unwrap();
        std::fs::write(d.join("a.txt"), b"x").unwrap();
        assert!(managed_file(&d, "a.wav").is_ok());
        assert!(managed_file(&d, "a.txt")
            .unwrap_err()
            .contains("not a .wav"));
        assert!(managed_file(&d, "../a.wav").is_err());
        assert!(managed_file(&d, "/etc/passwd")
            .unwrap_err()
            .contains("outside"));
        let other = scratch("managed-other");
        std::fs::write(other.join("b.wav"), b"x").unwrap();
        std::os::unix::fs::symlink(other.join("b.wav"), d.join("link.wav")).unwrap();
        assert!(managed_file(&d, "link.wav")
            .unwrap_err()
            .contains("outside"));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }

    // Tests [@ANCHOR: audio_store_retention]
    #[test]
    fn the_oldest_files_beyond_the_count_and_the_age_are_removed() {
        let d = scratch("prune");
        for i in 0..6 {
            let p = d.join(format!("rec-{i}.wav"));
            std::fs::write(&p, b"x").unwrap();
            let age = Duration::from_secs((6 - i) * 3600);
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(SystemTime::now() - age).unwrap();
        }
        std::fs::write(d.join("keep.json"), b"{}").unwrap();
        let reference = d.join("ref-door-beep.wav");
        std::fs::write(&reference, b"x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&reference)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(90 * 86400))
            .unwrap();
        assert_eq!(prune(&d, 4, 30), 2);
        assert!(reference.exists(), "reference recordings are not pruned");
        assert!(
            !d.join("rec-0.wav").exists()
                && !d.join("rec-1.wav").exists()
                && d.join("rec-5.wav").exists()
        );
        assert!(d.join("keep.json").exists());
        let old = d.join("rec-5.wav");
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(40 * 86400))
            .unwrap();
        assert_eq!(prune(&d, 10, 14), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn new_paths_are_unique_private_and_labelled_safely() {
        let d = scratch("new").join("sub");
        let a = new_path(&d, "rec", Some("relay ../click!")).unwrap();
        std::fs::write(&a, b"x").unwrap();
        let b = new_path(&d, "rec", Some("relay ../click!")).unwrap();
        assert_ne!(a, b);
        assert!(a
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("relay----click"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }
}
