// SPDX-License-Identifier: MIT OR Apache-2.0
//! Message decoration. Colors are ANSI codes, written only to a terminal, and never when
//! `NO_COLOR` is set to anything but the empty string (<https://no-color.org>): a pipe, a file or
//! a log gets the plain text.

use std::io::IsTerminal;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const BLUE: &str = "\x1b[34m";
const RESET: &str = "\x1b[0m";

/// The stream a message is written to, which decides whether it is colored.
#[derive(Clone, Copy)]
pub enum Stream {
    Out,
    Err,
}

fn colors(stream: Stream) -> bool {
    let wanted = std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty());
    wanted
        && match stream {
            Stream::Out => std::io::stdout().is_terminal(),
            Stream::Err => std::io::stderr().is_terminal(),
        }
}

fn paint<S: std::fmt::Display>(stream: Stream, color: &str, input: S) -> String {
    if colors(stream) {
        format!("{color}{input}{RESET}")
    } else {
        input.to_string()
    }
}

/// `[ ERROR ]: `, for standard error.
pub fn err_prefix() -> String {
    format!("{}: ", paint(Stream::Err, RED, "[ ERROR ]"))
}

/// `[ SUCCESS ]: `, for standard output.
pub fn success_prefix() -> String {
    format!("{}: ", paint(Stream::Out, GREEN, "[ SUCCESS ]"))
}

pub fn blue<S: std::fmt::Display>(stream: Stream, input: S) -> String {
    paint(stream, BLUE, input)
}

pub fn green<S: std::fmt::Display>(stream: Stream, input: S) -> String {
    paint(stream, GREEN, input)
}
