// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Detect Sixel support by asking the terminal itself, for terminals mdcat doesn't otherwise
//! recognize from `$TERM`/`$TERM_PROGRAM`.

/// Ask the terminal whether it supports Sixel graphics, by sending a Primary Device Attributes
/// (DA1) query (`CSI c`) and checking whether extension `4` ("Sixel graphics") is present in the
/// response.
///
/// This is the standards-documented way to detect Sixel support (see the DEC terminal reference
/// manuals, e.g. the VT340 programming manual's DA1 extension table), unlike heuristics based on
/// `$TERM`/`$TERM_PROGRAM`. It therefore also picks up Sixel support in terminals mdcat doesn't
/// otherwise recognize by name, such as forks or lesser-known emulators.
///
/// Blocks on terminal I/O for up to `timeout_ms` milliseconds. Returns `None` if the query
/// couldn't complete -- unsupported platform, timeout, or a response that doesn't look like a
/// DA1 reply -- since a silent or malformed response is not evidence either way, and callers
/// should treat that the same as "unsupported".
///
/// Callers should only call this once, at startup, on a terminal already confirmed to be
/// interactive (a real TTY), and not while already holding the terminal in raw mode themselves:
/// on Unix this function puts the terminal into raw mode for the duration of the query and
/// restores the previous mode afterwards.
pub fn detect_sixel_via_device_attributes(timeout_ms: u64) -> Option<bool> {
    #[cfg(unix)]
    {
        let _raw_mode = unix::RawModeGuard::enable().ok()?;
        query(timeout_ms)
    }
    #[cfg(windows)]
    {
        query(timeout_ms)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = timeout_ms;
        None
    }
}

#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
fn query(timeout_ms: u64) -> Option<bool> {
    let response = xterm_query::query("\x1b[c", timeout_ms).ok()?;
    Some(response_indicates_sixel(&response))
}

/// Check whether a Primary Device Attributes response indicates Sixel support.
///
/// A DA1 response has the form `CSI ? Pc ; Ps1 ; Ps2 ; ... c` (e.g.
/// `\x1b[?62;1;2;4;6;9;15;22c`), where `Pc` identifies the terminal class and each `Ps`
/// identifies a supported extension; `4` is Sixel graphics. `response` is the raw reply as
/// returned by the terminal, including the leading `ESC[` and any `?` private-marker prefix.
fn response_indicates_sixel(response: &str) -> bool {
    response
        .trim_start_matches(['\u{1b}', '['])
        .trim_end_matches('c')
        .trim_start_matches('?')
        .split(';')
        // The first parameter is the terminal class (`Pc`), not an extension.
        .skip(1)
        .any(|part| part == "4")
}

#[cfg(unix)]
mod unix {
    use rustix::termios::{tcgetattr, tcsetattr, OptionalActions, Termios};
    use std::fs::File;
    use std::io;

    /// Puts the controlling terminal into raw mode for the lifetime of this guard, restoring
    /// the previous settings on drop.
    pub(super) struct RawModeGuard {
        tty: File,
        original: Termios,
    }

    impl RawModeGuard {
        pub(super) fn enable() -> io::Result<Self> {
            let tty = File::open("/dev/tty")?;
            let original = tcgetattr(&tty)?;
            let mut raw = original.clone();
            raw.make_raw();
            tcsetattr(&tty, OptionalActions::Now, &raw)?;
            Ok(Self { tty, original })
        }
    }

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = tcsetattr(&self.tty, OptionalActions::Now, &self.original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::response_indicates_sixel;

    #[test]
    fn response_indicates_sixel_true_when_extension_4_present() {
        assert!(response_indicates_sixel("\u{1b}[?62;1;2;4;6;9;15;22c"));
    }

    #[test]
    fn response_indicates_sixel_false_when_extension_4_absent() {
        assert!(!response_indicates_sixel("\u{1b}[?62;1;2;6;9;15;22c"));
    }

    #[test]
    fn response_indicates_sixel_ignores_terminal_class_matching_4() {
        // The first parameter (`Pc`) is the terminal class, not an extension, so a class of `4`
        // alone must not be mistaken for Sixel support.
        assert!(!response_indicates_sixel("\u{1b}[?4;1;2c"));
    }

    #[test]
    fn response_indicates_sixel_false_for_malformed_response() {
        assert!(!response_indicates_sixel(""));
        assert!(!response_indicates_sixel("garbage"));
    }
}
