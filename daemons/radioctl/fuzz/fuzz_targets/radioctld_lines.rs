// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radioctld request parser, the attack surface of its network server: arbitrary bytes through
//! the bounded line buffer in two pushes, and every resulting text line through `parse_line`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use radioctld::command::{parse_line, Line, LineBuffer};

fuzz_target!(|data: &[u8]| {
    let (max, rest) = match data.split_first() {
        Some((max, rest)) => (usize::from(*max).max(1), rest),
        None => return,
    };
    let cut = rest.len() / 2;
    let mut buffer = LineBuffer::new(max);
    for chunk in [&rest[..cut], &rest[cut..]] {
        if let Ok(lines) = buffer.push(chunk) {
            for line in lines {
                if let Line::Text(text) = line {
                    let _ = parse_line(&text);
                }
            }
        }
    }
    if let Ok(text) = std::str::from_utf8(rest) {
        let _ = parse_line(text);
    }
});
