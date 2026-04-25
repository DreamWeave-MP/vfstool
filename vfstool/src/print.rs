// SPDX-License-Identifier: MIT OR Apache-2.0
const GREEN: &str = "\x1b[32m";
const BLUE: &str = "\x1b[34m";
const RESET: &str = "\x1b[0m";

pub const fn err_prefix() -> &'static str {
    concat!("\x1b[31m", "[ ERROR ]", "\x1b[0m", ": ")
}

pub const fn success_prefix() -> &'static str {
    concat!("\x1b[32m", "[ SUCCESS ]", "\x1b[0m", ": ")
}

pub fn blue<S: std::fmt::Display>(input: S) -> String {
    format!("{BLUE}{input}{RESET}")
}

pub fn green<S: std::fmt::Display>(input: S) -> String {
    format!("{GREEN}{input}{RESET}")
}
