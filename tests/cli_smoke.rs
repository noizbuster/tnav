use std::process::Command;

use tempfile::tempdir;

fn tnav_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tnav"))
}

#[test]
fn root_help_prints_usage() {
    let output = tnav_command()
        .arg("--help")
        .output()
        .expect("help command runs");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Interactive terminal navigation scaffold"));
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("tnav [OPTIONS] [COMMAND]"));
}

#[test]
fn version_subcommand_prints_package_version() {
    let output = tnav_command()
        .arg("version")
        .output()
        .expect("version command runs");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn bare_words_are_joined_as_prompt_flow() {
    let temp = tempdir().expect("tempdir");

    let output = tnav_command()
        .arg("show")
        .arg("current")
        .arg("directory")
        .env("XDG_CONFIG_HOME", temp.path())
        .output()
        .expect("question flow runs");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("run 'tnav connect' first"));
}
