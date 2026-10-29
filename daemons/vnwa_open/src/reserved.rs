// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which USB audio devices belong to the VNA and must be kept away from the sound server, by topology: a USB sound codec 08bb:29c0 is a VNWA codec
//! only if a sibling device under the same hub is the VNWA control chip 20a0:4118. The id 08bb:29c0 alone is the generic Texas Instruments PCM2900C
//! used by many USB sound dongles, so it must never be the whole test. Other software that picks audio devices should call `is_vnwa_codec`.

use std::path::Path;

/// The VNWA control chip.
pub const CONTROL_VID: &str = "20a0";
pub const CONTROL_PID: &str = "4118";
/// The codec chip (generic: see the module comment).
pub const CODEC_VID: &str = "08bb";
pub const CODEC_PID: &str = "29c0";

fn usb_id(dir: &Path) -> Option<(String, String)> {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).map(|s| s.trim().to_string());
    Some((read("idVendor").ok()?, read("idProduct").ok()?))
}

/// True if the USB device at `usb_path` (for example `3-2.2.2`, as under `/sys/bus/usb/devices`) is a PCM2900C that has a VNWA control chip
/// as a sibling on the same parent hub (`3-2.2.1`). `sys` is the sysfs root (`/sys`).
pub fn is_vnwa_codec(sys: &Path, usb_path: &str) -> bool {
    // [@ANCHOR: vnwa_open_reserved_codec]
    let devices = sys.join("bus/usb/devices");
    if usb_id(&devices.join(usb_path))
        .as_ref()
        .map(|(v, p)| (v.as_str(), p.as_str()))
        != Some((CODEC_VID, CODEC_PID))
    {
        return false;
    }
    let Some((parent, _)) = usb_path.rsplit_once('.') else {
        return false;
    };
    let Ok(rd) = std::fs::read_dir(&devices) else {
        return false;
    };
    rd.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name != usb_path
            && name.rsplit_once('.').map(|x| x.0) == Some(parent)
            && !name.contains(':')
            && usb_id(&e.path())
                .as_ref()
                .map(|(v, p)| (v.as_str(), p.as_str()))
                == Some((CONTROL_VID, CONTROL_PID))
    })
}

/// The USB paths of the VNWA codecs that are plugged in.
pub fn vnwa_codec_paths(sys: &Path) -> Vec<String> {
    crate::topology::find_cards(sys)
        .into_iter()
        .filter(|c| is_vnwa_codec(sys, &c.usb_path))
        .map(|c| c.usb_path)
        .collect()
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::Path;

    /// A fake sysfs with `/bus/usb/devices/<name>/{idVendor,idProduct}` for each (name, vid, pid).
    pub fn tree(root: &Path, devs: &[(&str, &str, &str)]) {
        for (name, v, p) in devs {
            let d = root.join("bus/usb/devices").join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("idVendor"), format!("{v}\n")).unwrap();
            std::fs::write(d.join("idProduct"), format!("{p}\n")).unwrap();
        }
    }

    pub fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("vnwa_open_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    // Tests [@ANCHOR: vnwa_open_reserved_codec]
    #[test]
    fn a_codec_is_the_vnwas_only_with_the_control_chip_beside_it_under_the_same_hub() {
        let root = tmp("reserved");
        // The unit under test: hub 3-2.2 with the control chip (.1) and two codecs (.2, .3); and a stand-alone dongle on 3-3 and another on hub 3-4.1.
        tree(
            &root,
            &[
                ("3-2.2", "0451", "2036"),
                ("3-2.2.1", "20a0", "4118"),
                ("3-2.2.2", "08bb", "29c0"),
                ("3-2.2.3", "08bb", "29c0"),
                ("3-3.1", "08bb", "29c0"),
                ("3-4.1.2", "08bb", "29c0"),
                ("3-4.1.1", "046d", "0825"),
            ],
        );
        assert!(is_vnwa_codec(&root, "3-2.2.2") && is_vnwa_codec(&root, "3-2.2.3"));
        assert!(
            !is_vnwa_codec(&root, "3-3.1"),
            "a dongle with no control chip beside it"
        );
        assert!(
            !is_vnwa_codec(&root, "3-4.1.2"),
            "a dongle beside some other device"
        );
        assert!(
            !is_vnwa_codec(&root, "3-2.2.1"),
            "the control chip is not a codec"
        );
        assert!(!is_vnwa_codec(&root, "3-9.9"), "absent");
        let _ = std::fs::remove_dir_all(&root);
    }
}
