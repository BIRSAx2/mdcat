// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Test the command line interface of mdcat

#![deny(warnings, clippy::all)]

mod cli {
    use std::ffi::OsStr;
    use std::io::{Read, Write};
    use std::process::{Command, Output, Stdio};

    fn cargo_mdcat() -> Command {
        Command::new(env!("CARGO_BIN_EXE_mdcat"))
    }

    fn run_cargo_mdcat<I, S>(args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        cargo_mdcat().args(args).output().unwrap()
    }

    #[test]
    fn show_help() {
        let output = run_cargo_mdcat(["--help"]);
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(
            output.status.success(),
            "non-zero exit code: {:?}",
            output.status,
        );
        assert!(output.stderr.is_empty());
        assert!(stdout.contains("See 'man 1 mdcat' for more information."));
    }

    #[test]
    fn long_version_includes_license() {
        let output = run_cargo_mdcat(["--version"]);
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(
            output.status.success(),
            "non-zero exit code: {:?}",
            output.status,
        );
        assert!(output.stderr.is_empty());
        assert!(
            stdout.contains("This program is subject to the terms of the Mozilla Public License,")
        );
    }

    #[test]
    fn file_list_fail_late() {
        let output = run_cargo_mdcat(["does-not-exist", "sample/common-mark.md"]);
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(!output.status.success());
        // We failed to read the first file but still printed the second.
        assert!(
            stderr.contains("Error: does-not-exist:") && stderr.contains("(os error 2)"),
            "Stderr: {stderr}",
        );
        assert!(stdout.contains("CommonMark sample document"));
    }

    #[test]
    fn file_list_fail_fast() {
        let output = run_cargo_mdcat(["--fail", "does-not-exist", "sample/common-mark.md"]);
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        assert!(!output.status.success());
        // We failed to read the first file and exited early, so nothing was printed at all
        assert!(
            stderr.contains("Error: does-not-exist:") && stderr.contains("(os error 2)"),
            "Stderr: {stderr}",
        );
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn toc_lists_headings_before_content() {
        let output = run_cargo_mdcat(["--no-colour", "--toc", "sample/common-mark.md"]);
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(output.status.success());
        let toc_pos = stdout
            .find("Table of Contents")
            .expect("TOC heading missing");
        let content_pos = stdout
            .find("CommonMark sample document")
            .expect("document content missing");
        assert!(
            toc_pos < content_pos,
            "TOC must come before the document content"
        );
        assert!(stdout.contains("common-mark.md#basic-inline-formatting"));
    }

    #[test]
    fn toc_on_stdin_has_no_links() {
        let mut child = cargo_mdcat()
            .args(["--no-colour", "--toc", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "# One\n\n# Two\n").unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(output.status.success());
        assert!(stdout.contains("Table of Contents"));
        assert!(!stdout.contains(".md#"));
    }

    fn image_markdown() -> String {
        let image = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("sample")
            .join("rust-logo-128x128.png");
        // Embed a proper `file://` URL rather than a native path: on Windows, a bare
        // `D:\...` path isn't valid relative-reference syntax, so resolving it as a
        // Markdown link destination fails and the image silently falls back to a
        // plain link instead of actually rendering.
        let url = url::Url::from_file_path(&image).expect("absolute path");
        format!("![alt]({url})\n")
    }

    #[test]
    fn image_protocol_flag_forces_kitty_escape_sequence() {
        let mut child = cargo_mdcat()
            .args(["--ansi", "--image-protocol=kitty", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{}", image_markdown()).unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(output.status.success());
        assert!(
            stdout.contains("\x1b_G"),
            "expected a kitty graphics escape sequence, got: {stdout:?}"
        );
    }

    #[test]
    fn image_protocol_env_var_forces_iterm2_escape_sequence() {
        let mut child = cargo_mdcat()
            .args(["--ansi", "-"])
            .env("MDCAT_IMAGE_PROTOCOL", "iterm2")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{}", image_markdown()).unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(output.status.success());
        assert!(
            stdout.contains("\x1b]1337;File="),
            "expected an iTerm2 inline image escape sequence, got: {stdout:?}"
        );
    }

    #[test]
    fn image_protocol_flag_overrides_env_var() {
        let mut child = cargo_mdcat()
            .args(["--ansi", "--image-protocol=kitty", "-"])
            .env("MDCAT_IMAGE_PROTOCOL", "none")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "{}", image_markdown()).unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        assert!(output.status.success());
        assert!(
            stdout.contains("\x1b_G"),
            "explicit --image-protocol should win over $MDCAT_IMAGE_PROTOCOL, got: {stdout:?}"
        );
    }

    #[test]
    fn image_protocol_config_default_is_used_and_overridable() {
        let config_dir = std::env::temp_dir().join(format!(
            "mdcat-cli-test-config-{}-{}",
            std::process::id(),
            "image_protocol_config_default_is_used_and_overridable"
        ));
        std::fs::create_dir_all(config_dir.join("mdcat")).unwrap();
        std::fs::write(
            config_dir.join("mdcat/config.toml"),
            "[defaults]\nimage_protocol = \"kitty\"\n",
        )
        .unwrap();

        let run = |extra_args: &[&str]| {
            let mut child = cargo_mdcat()
                .args(["--ansi"])
                .args(extra_args)
                .arg("-")
                .env("XDG_CONFIG_HOME", &config_dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            write!(child.stdin.take().unwrap(), "{}", image_markdown()).unwrap();
            let output = child.wait_with_output().unwrap();
            String::from_utf8(output.stdout).unwrap()
        };

        let from_config = run(&[]);
        assert!(
            from_config.contains("\x1b_G"),
            "config default should force kitty, got: {from_config:?}"
        );

        let overridden = run(&["--image-protocol=none"]);
        assert!(
            !overridden.contains("\x1b_G"),
            "explicit flag should override the config default, got: {overridden:?}"
        );

        std::fs::remove_dir_all(&config_dir).unwrap();
    }

    #[test]
    fn ignore_broken_pipe() {
        let mut child = cargo_mdcat()
            .stdin(Stdio::piped())
            // .arg("sample/common-mark.md")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let mut stdin = child.stdin.take().unwrap();
        let mut stderr = Vec::new();
        drop(child.stdout.take());

        writeln!(stdin, "Hello world").unwrap();
        drop(stdin);
        child
            .stderr
            .as_mut()
            .unwrap()
            .read_to_end(&mut stderr)
            .unwrap();
        let exit_code = child.wait().unwrap();

        similar_asserts::assert_eq!(String::from_utf8_lossy(&stderr), "");
        assert_eq!(exit_code.code().unwrap(), 0);
    }
}
