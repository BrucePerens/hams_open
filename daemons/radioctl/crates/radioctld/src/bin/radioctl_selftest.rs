// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-selftest`: see `radioctld::selftest_cli`.

fn main() {
    std::process::exit(radioctld::selftest_cli::main_from_env());
}
