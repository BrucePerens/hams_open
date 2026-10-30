// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shipped example procedures. They are program text embedded in the server (`examples/*.benchproc`) and appear next to the
//! saved procedures under reserved names that start with `example-`: `bench_proc_load` returns their source, `bench_proc_run`
//! with `name` runs them, and nobody can save or delete a name that starts with `example-`. Each is parameterised by
//! `bench.args` with the real bench's tools as defaults; the tests run the same text against a mock bench. The manual page of each
//! (`procedures/`) holds the same text, and a test fails if the two differ.

/// The prefix of the reserved names.
pub const PREFIX: &str = "example-";

pub struct Example {
    pub name: &'static str,
    pub tier: &'static str,
    pub description: &'static str,
    pub source: &'static str,
}

pub const SHIPPED: &[Example] = &[
    Example {
        name: "example-sweep-series",
        tier: "read_config",
        description: "Takes the same VNA sweep several times with retries and a power cycle after failures in a row, and tabulates the spread of the magnitude.",
        source: include_str!("examples/sweep_series.benchproc"),
    },
    Example {
        name: "example-soak-round",
        tier: "read_config",
        description: "Rounds of generator steps read on the scope and the VNA and compared with a baseline; counts ERROR and WRONG.",
        source: include_str!("examples/soak_round.benchproc"),
    },
    Example {
        name: "example-calibration-sequence",
        tier: "read_config",
        description: "The VNA calibration with operator prompts: connect each standard, answer through bench_proc_input, measure, build.",
        source: include_str!("examples/calibration_sequence.benchproc"),
    },
];

/// The shipped example of this name, if there is one.
pub fn find(name: &str) -> Option<&'static Example> {
    // [@ANCHOR: proc_examples_find]
    SHIPPED.iter().find(|e| e.name == name)
}
