use std::process::Command;

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
    assert!(stdout.contains("tnav [OPTIONS] <COMMAND>"));
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
fn invalid_subcommand_exits_with_clap_error() {
    let output = tnav_command()
        .arg("bogus")
        .output()
        .expect("invalid command runs");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unrecognized subcommand 'bogus'"));
    assert!(stderr.contains("Usage:"));
}
