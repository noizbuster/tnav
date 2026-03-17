use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::NamedTempFile;

use crate::errors::TnavError;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionPlan {
    ShellCommand {
        program: OsString,
        args: Vec<OsString>,
    },
    ScriptFile {
        program: OsString,
        args: Vec<OsString>,
        script_body: String,
    },
}

pub fn execute_command(command: &str) -> Result<(), TnavError> {
    match execution_plan(command) {
        ExecutionPlan::ShellCommand { program, args } => {
            run_process(Command::new(program).args(args).arg(command))
        }
        ExecutionPlan::ScriptFile {
            program,
            args,
            script_body,
        } => {
            let mut script_file = create_script_file(&script_body)?;
            let script_path = script_file.path().to_owned();
            script_file
                .flush()
                .map_err(|error| TnavError::CommandFailed {
                    message: format!("failed to flush shell script: {error}"),
                })?;

            run_process(Command::new(program).args(args).arg(script_path))
        }
    }
}

fn run_process(command: &mut Command) -> Result<(), TnavError> {
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| TnavError::CommandFailed {
            message: format!("failed to execute shell command: {error}"),
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(TnavError::CommandFailed {
            message: format!("shell command exited with status {status}"),
        })
    }
}

fn execution_plan(command: &str) -> ExecutionPlan {
    if let Some((program, args)) = parse_shebang(command) {
        return ExecutionPlan::ScriptFile {
            program,
            args,
            script_body: command.to_owned(),
        };
    }

    let (program, args) = current_shell_command();
    ExecutionPlan::ShellCommand { program, args }
}

fn create_script_file(command: &str) -> Result<NamedTempFile, TnavError> {
    let mut file = NamedTempFile::new().map_err(|error| TnavError::CommandFailed {
        message: format!("failed to create temporary shell script: {error}"),
    })?;
    file.write_all(command.as_bytes())
        .map_err(|error| TnavError::CommandFailed {
            message: format!("failed to write temporary shell script: {error}"),
        })?;
    Ok(file)
}

fn current_shell_command() -> (OsString, Vec<OsString>) {
    #[cfg(windows)]
    {
        let program = env::var_os("COMSPEC").unwrap_or_else(|| OsString::from("cmd.exe"));
        return (program, vec![OsString::from("/C")]);
    }

    #[cfg(not(windows))]
    {
        let program = env::var_os("SHELL").filter(|value| !value.is_empty());
        (
            program.unwrap_or_else(|| OsString::from("sh")),
            vec![OsString::from("-c")],
        )
    }
}

fn parse_shebang(command: &str) -> Option<(OsString, Vec<OsString>)> {
    let first_line = command.lines().next()?.trim();
    let interpreter = first_line.strip_prefix("#!")?.trim();
    if interpreter.is_empty() {
        return None;
    }

    let mut tokens = interpreter.split_whitespace();
    let program = OsString::from(tokens.next()?);
    let args = tokens.map(OsString::from).collect::<Vec<_>>();
    Some((program, args))
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use std::ffi::OsString;

    use super::{
        ExecutionPlan, current_shell_command, execute_command, execution_plan, parse_shebang,
    };

    #[test]
    fn parse_shebang_reads_direct_interpreter() {
        let (program, args) = parse_shebang("#!/bin/zsh\nprint hello").expect("shebang parses");

        assert_eq!(program, OsString::from("/bin/zsh"));
        assert!(args.is_empty());
    }

    #[test]
    fn parse_shebang_keeps_env_arguments() {
        let (program, args) =
            parse_shebang("#!/usr/bin/env zsh -f\nprint hello").expect("shebang parses");

        assert_eq!(program, OsString::from("/usr/bin/env"));
        assert_eq!(args, vec![OsString::from("zsh"), OsString::from("-f")]);
    }

    #[test]
    fn execution_plan_prefers_shebang_script_execution() {
        let plan = execution_plan("#!/bin/zsh\n[[ -n foo ]]\nprint foo");

        assert_eq!(
            plan,
            ExecutionPlan::ScriptFile {
                program: OsString::from("/bin/zsh"),
                args: Vec::new(),
                script_body: "#!/bin/zsh\n[[ -n foo ]]\nprint foo".to_owned(),
            }
        );
    }

    #[test]
    fn execution_plan_uses_current_shell_for_plain_commands() {
        let plan = execution_plan("printf '%s\\n' hello");
        let (program, args) = current_shell_command();

        assert_eq!(plan, ExecutionPlan::ShellCommand { program, args });
    }

    #[test]
    fn execute_command_honors_bash_shebang_for_bash_syntax() {
        let bash_available = Command::new("bash")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);

        if !bash_available {
            return;
        }

        execute_command("#!/usr/bin/env bash\n[[ -n foo ]]\n")
            .expect("bash shebang script should execute successfully");
    }
}
