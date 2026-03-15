use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::errors::TnavError;

const ANSI_RED: &str = "\x1b[31m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy)]
pub struct Output {
    json: bool,
    quiet: bool,
}

impl Output {
    pub fn new(global: &GlobalArgs) -> Self {
        Self {
            json: global.json,
            quiet: global.quiet,
        }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    pub fn print_json<T>(&self, value: &T) -> Result<(), TnavError>
    where
        T: Serialize,
    {
        let json =
            serde_json::to_string_pretty(value).map_err(|error| TnavError::CommandFailed {
                message: format!("failed to serialize JSON output: {error}"),
            })?;
        println!("{json}");
        Ok(())
    }

    pub fn line(&self, message: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", message.as_ref());
        }
    }

    pub fn green_heading(&self, message: impl AsRef<str>) {
        self.colored_heading(message, ANSI_GREEN);
    }

    pub fn yellow_heading(&self, message: impl AsRef<str>) {
        self.colored_heading(message, ANSI_YELLOW);
    }

    pub fn box_message(&self, message: impl AsRef<str>) {
        if self.quiet {
            return;
        }

        println!("{}", format_box(message.as_ref()));
    }

    pub fn command_preview(&self, command: impl AsRef<str>) -> usize {
        if self.quiet {
            return 0;
        }

        let rendered = render_command_preview(command.as_ref());
        let line_count = rendered.lines().count();
        println!("{rendered}");
        line_count
    }

    pub fn clear_rendered_lines(&self, line_count: usize) {
        if self.quiet || line_count == 0 {
            return;
        }

        print!("\x1b[{}F\x1b[J", line_count);
        let _ = io::stdout().flush();
    }

    pub fn red_box(&self, message: impl AsRef<str>) {
        if self.quiet {
            return;
        }

        println!("{}{}{}", ANSI_RED, format_box(message.as_ref()), ANSI_RESET);
    }

    pub fn start_progress(&self, message: impl Into<String>) -> Option<ProgressHandle> {
        if self.quiet || self.json {
            return None;
        }

        Some(ProgressHandle::start(message.into()))
    }

    fn colored_heading(&self, message: impl AsRef<str>, color: &str) {
        if self.quiet {
            return;
        }

        if self.json {
            println!("{}", message.as_ref());
        } else {
            println!("{color}{}{ANSI_RESET}", message.as_ref());
        }
    }
}

#[derive(Debug)]
pub struct ProgressHandle {
    active: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressHandle {
    fn start(message: String) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let thread_active = Arc::clone(&active);
        let handle = thread::spawn(move || {
            let frames = ["-", "\\", "|", "/"];
            let mut frame_index = 0usize;
            let mut stderr = io::stderr();

            while thread_active.load(Ordering::Relaxed) {
                let frame = frames[frame_index % frames.len()];
                let _ = write!(stderr, "\r{} {}", message, frame);
                let _ = stderr.flush();
                frame_index += 1;
                thread::sleep(Duration::from_millis(90));
            }

            let clear_width = message.len() + 4;
            let _ = write!(stderr, "\r{}\r", " ".repeat(clear_width));
            let _ = stderr.flush();
        });

        Self {
            active,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProgressHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn format_box(message: &str) -> String {
    let lines = message.lines().collect::<Vec<_>>();
    let width = lines.iter().map(|line| line.len()).max().unwrap_or(0);
    let border = format!("+{}+", "-".repeat(width + 2));

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(border.clone());
    for line in lines {
        rendered.push(format!("| {:width$} |", line, width = width));
    }
    rendered.push(border);

    rendered.join("\n")
}

fn render_command_preview(command: &str) -> String {
    let lines = if command.is_empty() {
        vec![""]
    } else {
        command.lines().collect::<Vec<_>>()
    };
    let rule_width = lines
        .iter()
        .map(|line| line.len())
        .max()
        .unwrap_or(0)
        .max(24);
    let rule = "-".repeat(rule_width);

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(rule.clone());
    rendered.extend(
        lines
            .into_iter()
            .map(|line| format!("{ANSI_CYAN}{line}{ANSI_RESET}")),
    );
    rendered.push(rule);

    rendered.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{ANSI_CYAN, ANSI_RESET, format_box, render_command_preview};

    #[test]
    fn format_box_wraps_single_line_message() {
        let rendered = format_box("No LLM provider is configured yet.");

        assert_eq!(
            rendered,
            "+------------------------------------+\n| No LLM provider is configured yet. |\n+------------------------------------+"
        );
    }

    #[test]
    fn format_box_pads_multiple_lines_to_same_width() {
        let rendered = format_box("alpha\nbeta beta");

        assert_eq!(
            rendered,
            "+-----------+\n| alpha     |\n| beta beta |\n+-----------+"
        );
    }

    #[test]
    fn format_box_keeps_multiline_script_shape() {
        let rendered = format_box("set -euo pipefail\nmkdir -p out\nprintf 'ok\\n'");

        assert_eq!(
            rendered,
            "+-------------------+\n| set -euo pipefail |\n| mkdir -p out      |\n| printf 'ok\\n'     |\n+-------------------+"
        );
    }

    #[test]
    fn render_command_preview_uses_rules_and_colored_lines() {
        let rendered = render_command_preview("pwd\nls");

        assert_eq!(
            rendered,
            format!(
                "------------------------\n{ANSI_CYAN}pwd{ANSI_RESET}\n{ANSI_CYAN}ls{ANSI_RESET}\n------------------------"
            )
        );
    }
}
