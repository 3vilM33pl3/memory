// SPDX-License-Identifier: AGPL-3.0-or-later

fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
