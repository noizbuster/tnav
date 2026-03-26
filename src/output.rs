use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use unicode_width::UnicodeWidthChar;

use crate::cli::GlobalArgs;
use crate::errors::TnavError;

const ANSI_RED: &str = "\x1b[31m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RESET: &str = "\x1b[0m";
const TAB_STOP_WIDTH: usize = 8;

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
        self.command_preview_with_notice(command, None)
    }

    pub fn command_preview_with_notice(
        &self,
        command: impl AsRef<str>,
        notice: Option<&str>,
    ) -> usize {
        if self.quiet {
            return 0;
        }

        let rendered = match notice {
            Some(notice) => render_command_preview_with_notice(command.as_ref(), Some(notice)),
            None => render_command_preview(command.as_ref()),
        };
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
            println!("{color}{}{ANSI_RESET}", format_heading(message.as_ref()));
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
    let width = lines
        .iter()
        .map(|line| visible_width(line))
        .max()
        .unwrap_or(0);
    let border = format!("+{}+", "-".repeat(width + 2));

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(border.clone());
    for line in lines {
        rendered.push(render_box_line(line, width));
    }
    rendered.push(border);

    rendered.join("\n")
}

fn format_heading(message: &str) -> String {
    format!("== {message} ==")
}

fn render_command_preview(command: &str) -> String {
    render_command_preview_with_notice(command, None)
}

fn render_command_preview_with_notice(command: &str, notice: Option<&str>) -> String {
    let lines = if command.is_empty() {
        vec![""]
    } else {
        command.lines().collect::<Vec<_>>()
    };
    let notice_lines = notice
        .map(|message| message.lines().collect::<Vec<_>>())
        .unwrap_or_default();
    let width = lines
        .iter()
        .chain(notice_lines.iter())
        .map(|line| visible_width(line))
        .max()
        .unwrap_or(0)
        .max(visible_width("Command preview"))
        .max(24);
    let border = format!("+{}+", "-".repeat(width + 2));

    let mut rendered = Vec::with_capacity(lines.len() + notice_lines.len() + 4);
    rendered.push(border.clone());
    rendered.push(render_box_line("Command preview", width));
    rendered.extend(
        notice_lines
            .into_iter()
            .map(|line| render_box_line(line, width)),
    );
    rendered.push(border.clone());
    rendered.extend(lines.into_iter().map(|line| {
        let padded = pad_to_visible_width(&expand_tabs(line), width);
        format!("| {ANSI_CYAN}{padded}{ANSI_RESET} |")
    }));
    rendered.push(border);

    rendered.join("\n")
}

fn render_box_line(line: &str, width: usize) -> String {
    let padded = pad_to_visible_width(&expand_tabs(line), width);
    format!("| {padded} |")
}

fn pad_to_visible_width(line: &str, width: usize) -> String {
    let padding = width.saturating_sub(visible_width(line));
    format!("{line}{}", " ".repeat(padding))
}

fn expand_tabs(line: &str) -> String {
    let mut expanded = String::new();
    let mut width = 0usize;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\t' => {
                let tab_width = TAB_STOP_WIDTH - (width % TAB_STOP_WIDTH);
                expanded.push_str(&" ".repeat(tab_width));
                width += tab_width;
            }
            '\u{1b}' if matches!(chars.peek(), Some('[')) => {
                expanded.push(ch);
                expanded.push(chars.next().expect("CSI introducer"));
                for next in chars.by_ref() {
                    expanded.push(next);
                    if matches!(next, '\u{40}'..='\u{7e}') {
                        break;
                    }
                }
            }
            _ => {
                expanded.push(ch);
                width += UnicodeWidthChar::width(ch).unwrap_or(0);
            }
        }
    }

    expanded
}

fn visible_width(line: &str) -> usize {
    let mut width = 0usize;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\t' => {
                let tab_width = TAB_STOP_WIDTH - (width % TAB_STOP_WIDTH);
                width += tab_width;
            }
            '\u{1b}' if matches!(chars.peek(), Some('[')) => {
                chars.next();
                for next in chars.by_ref() {
                    if matches!(next, '\u{40}'..='\u{7e}') {
                        break;
                    }
                }
            }
            _ => width += UnicodeWidthChar::width(ch).unwrap_or(0),
        }
    }

    width
}

#[cfg(test)]
mod tests {
    use super::{
        ANSI_CYAN, ANSI_RESET, expand_tabs, format_box, format_heading, pad_to_visible_width,
        render_command_preview, render_command_preview_with_notice, visible_width,
    };

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
    fn format_heading_wraps_sections_consistently() {
        assert_eq!(format_heading("Saved providers"), "== Saved providers ==");
    }

    #[test]
    fn render_command_preview_uses_a_titled_box_and_colored_lines() {
        let rendered = render_command_preview("pwd\nls");

        assert_eq!(
            rendered,
            format!(
                "+--------------------------+\n| Command preview          |\n+--------------------------+\n| {ANSI_CYAN}pwd                     {ANSI_RESET} |\n| {ANSI_CYAN}ls                      {ANSI_RESET} |\n+--------------------------+"
            )
        );
    }

    #[test]
    fn render_command_preview_places_notice_inside_the_preview_box() {
        let notice = "Reusing the saved response from history.";
        let rendered = render_command_preview_with_notice("pwd", Some(notice));
        let lines = rendered.lines().collect::<Vec<_>>();

        assert!(lines[1].starts_with("| Command preview"));
        assert_eq!(lines[2], "| Reusing the saved response from history. |");
        assert!(!rendered.starts_with(notice));
    }

    #[test]
    fn visible_width_ignores_ansi_sequences_and_expands_tabs() {
        let styled = format!("{ANSI_CYAN}\talpha{ANSI_RESET}");

        assert_eq!(visible_width(&styled), 13);
    }

    #[test]
    fn expand_tabs_and_padding_align_tabbed_lines_for_terminal_output() {
        let padded = pad_to_visible_width(&expand_tabs("\talpha"), 16);

        assert_eq!(visible_width(&padded), 16);
        assert!(padded.starts_with("        alpha"));
    }

    #[test]
    fn render_command_preview_keeps_borders_aligned_for_tabbed_lines() {
        let rendered = render_command_preview("printf 'name\tcount\\n'\n\tawk '{print $1}'");
        let border_width = visible_width(rendered.lines().next().expect("top border"));

        for line in rendered.lines() {
            assert_eq!(visible_width(&strip_ansi(line)), border_width);
        }
    }

    fn strip_ansi(text: &str) -> String {
        let mut stripped = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
                chars.next();
                for next in chars.by_ref() {
                    if matches!(next, '\u{40}'..='\u{7e}') {
                        break;
                    }
                }
                continue;
            }

            stripped.push(ch);
        }

        stripped
    }
}
