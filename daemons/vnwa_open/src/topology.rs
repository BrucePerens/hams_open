// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which ALSA card is which codec. The cards are named in the order the kernel enumerated them (`CODEC`, `CODEC_1`), and that
//! order can change when the unit is power-cycled, so the S11 and S21 codecs are told apart by where they sit on the USB tree:
//! the unit's internal hub gives them fixed ports. Observed 2026-10-28: the S11 codec (reference on its left channel) is on
//! port 2 of the unit's hub and the S21 codec (reference on its right channel) on port 3.

use std::path::Path;

/// The audio codec chip on the unit (Texas Instruments PCM2900C).
pub const CODEC_VID: &str = "08bb";
pub const CODEC_PID: &str = "29c0";

/// One ALSA card on a USB codec: its card id and its USB port path (for example `3-2.2.3`).
#[derive(Clone, Debug, PartialEq)]
pub struct CodecCard {
    pub card_id: String,
    pub usb_path: String,
}

/// The ALSA ids (`hw:CARD=<id>,`) of the two codecs.
#[derive(Clone, Debug, PartialEq)]
pub struct Codecs {
    pub s11: String,
    pub s21: String,
}

fn last_port(path: &str) -> Option<u32> {
    path.rsplit(['.', '-']).next()?.parse().ok()
}

/// Picks the S11 and S21 codecs from the codec cards found: they must share one parent hub, and the lower port is the S11 one.
pub fn assign(cards: &[CodecCard]) -> Result<Codecs, String> {
    // [@ANCHOR: vnwa_open_codec_topology]
    if cards.len() != 2 {
        return Err(format!(
            "expected the unit's two USB codecs, found {} ({})",
            cards.len(),
            cards
                .iter()
                .map(|c| format!("{} at {}", c.card_id, c.usb_path))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let parent = |p: &str| p.rsplit_once('.').map(|x| x.0.to_string());
    if parent(&cards[0].usb_path) != parent(&cards[1].usb_path) {
        return Err(format!(
            "the two codecs {} and {} are not on the same hub",
            cards[0].usb_path, cards[1].usb_path
        ));
    }
    let (a, b) = (
        last_port(&cards[0].usb_path).ok_or("unreadable USB path")?,
        last_port(&cards[1].usb_path).ok_or("unreadable USB path")?,
    );
    if a == b {
        return Err("the two codecs report the same USB port".into());
    }
    let (lo, hi) = if a < b {
        (&cards[0], &cards[1])
    } else {
        (&cards[1], &cards[0])
    };
    Ok(Codecs {
        s11: format!("hw:CARD={},", lo.card_id),
        s21: format!("hw:CARD={},", hi.card_id),
    })
}

/// Reads the codec cards from sysfs (`/sys/class/sound/card*`), keeping those whose USB device is the PCM2900C.
pub fn find_cards(sys: &Path) -> Vec<CodecCard> {
    // [@ANCHOR: vnwa_open_codec_find_cards]
    let mut found = Vec::new();
    let Ok(dir) = std::fs::read_dir(sys.join("class/sound")) else {
        return found;
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") {
            continue;
        }
        let Ok(card_id) = std::fs::read_to_string(entry.path().join("id")) else {
            continue;
        };
        let Ok(dev) = std::fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        // .../3-2.2.3/3-2.2.3:1.0 -> the USB device is the parent of the interface directory.
        let Some(usb) = dev.parent() else { continue };
        let read = |f: &str| {
            std::fs::read_to_string(usb.join(f))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        if read("idVendor") == CODEC_VID && read("idProduct") == CODEC_PID {
            if let Some(p) = usb.file_name() {
                found.push(CodecCard {
                    card_id: card_id.trim().to_string(),
                    usb_path: p.to_string_lossy().to_string(),
                });
            }
        }
    }
    found
}

/// The codecs of the unit that is plugged in now.
pub fn codecs() -> Result<Codecs, String> {
    assign(&find_cards(Path::new("/sys")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, path: &str) -> CodecCard {
        CodecCard {
            card_id: id.into(),
            usb_path: path.into(),
        }
    }

    // Tests [@ANCHOR: vnwa_open_codec_topology]
    #[test]
    fn the_codec_on_the_lower_port_is_s11_whatever_the_card_order() {
        // As observed on the unit: CODEC at 3-2.2.2 and CODEC_1 at 3-2.2.3.
        let a = assign(&[card("CODEC", "3-2.2.2"), card("CODEC_1", "3-2.2.3")]).unwrap();
        assert_eq!(
            (a.s11.as_str(), a.s21.as_str()),
            ("hw:CARD=CODEC,", "hw:CARD=CODEC_1,")
        );
        // After a power cycle the kernel may enumerate them the other way round.
        let b = assign(&[card("CODEC", "3-2.2.3"), card("CODEC_1", "3-2.2.2")]).unwrap();
        assert_eq!(
            (b.s11.as_str(), b.s21.as_str()),
            ("hw:CARD=CODEC_1,", "hw:CARD=CODEC,")
        );
        // The order of the list does not matter either.
        let c = assign(&[card("CODEC_1", "3-2.2.2"), card("CODEC", "3-2.2.3")]).unwrap();
        assert_eq!(c, b);
    }

    #[test]
    fn a_missing_codec_or_codecs_on_different_hubs_is_a_clear_error() {
        assert!(assign(&[card("CODEC", "3-2.2.2")])
            .unwrap_err()
            .contains("found 1"));
        assert!(assign(&[]).unwrap_err().contains("found 0"));
        assert!(
            assign(&[card("CODEC", "3-2.2.2"), card("CODEC_1", "3-4.1.3")])
                .unwrap_err()
                .contains("not on the same hub")
        );
        assert!(assign(&[card("A", "3-2.2.2"), card("B", "3-2.2.2")])
            .unwrap_err()
            .contains("same USB port"));
    }

    // Tests [@ANCHOR: vnwa_open_codec_find_cards]
    #[test]
    fn sysfs_is_read_for_the_codec_vendor_only() {
        let root = std::env::temp_dir().join(format!("vnwa_topo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (card, id, usb, vid, pid) in [
            ("card1", "CODEC", "3-2.2.2", "08bb", "29c0"),
            ("card2", "CODEC_1", "3-2.2.3", "08bb", "29c0"),
            ("card3", "C920", "3-3.1.3", "046d", "082d"),
        ] {
            let usbdir = root.join("devices").join(usb);
            let iface = usbdir.join(format!("{usb}:1.0"));
            std::fs::create_dir_all(&iface).unwrap();
            std::fs::write(usbdir.join("idVendor"), format!("{vid}\n")).unwrap();
            std::fs::write(usbdir.join("idProduct"), format!("{pid}\n")).unwrap();
            let c = root.join("class/sound").join(card);
            std::fs::create_dir_all(&c).unwrap();
            std::fs::write(c.join("id"), format!("{id}\n")).unwrap();
            std::os::unix::fs::symlink(&iface, c.join("device")).unwrap();
        }
        let mut cards = find_cards(&root);
        cards.sort_by(|a, b| a.card_id.cmp(&b.card_id));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            cards,
            vec![card("CODEC", "3-2.2.2"), card("CODEC_1", "3-2.2.3")]
        );
    }
}
