use std::io::{self, Write};

use crate::history::HistoryEntry;

use crossterm::{
    cursor::{MoveLeft, MoveRight, MoveToColumn},
    event::{Event, KeyCode, KeyModifiers, read},
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct Readline<'a> {
    prompt: &'a str,
    history: &'a [HistoryEntry],
}

pub struct ReadlineResult {
    pub line: String,
    pub history_entry_id: Option<String>,
    pub modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CursorState {
    byte_pos: usize,
    char_index: usize,
    display_width: usize,
}

impl CursorState {
    fn new(_input: &str) -> Self {
        Self::default()
    }

    fn from_byte_pos(input: &str, byte_pos: usize) -> Option<Self> {
        if byte_pos > input.len() || !input.is_char_boundary(byte_pos) {
            return None;
        }

        let prefix = input.get(..byte_pos)?;
        let char_index = prefix.chars().count();
        let display_width = prefix.width();
        Some(Self {
            byte_pos,
            char_index,
            display_width,
        })
    }

    fn at_end(input: &str) -> Self {
        Self {
            byte_pos: input.len(),
            char_index: input.chars().count(),
            display_width: input.width(),
        }
    }

    fn to_byte_pos(self) -> usize {
        self.byte_pos
    }

    #[cfg(test)]
    fn char_index(self) -> usize {
        self.char_index
    }

    fn display_width(self) -> usize {
        self.display_width
    }

    fn move_left(&mut self, input: &str) -> bool {
        if self.byte_pos == 0 {
            return false;
        }

        let previous = previous_char_boundary(input, self.byte_pos);
        if let Some(updated) = Self::from_byte_pos(input, previous) {
            *self = updated;
            true
        } else {
            false
        }
    }

    fn move_right(&mut self, input: &str) -> bool {
        if self.byte_pos >= input.len() {
            return false;
        }

        let next = next_char_boundary(input, self.byte_pos);
        if let Some(updated) = Self::from_byte_pos(input, next) {
            *self = updated;
            true
        } else {
            false
        }
    }

    fn advance_by_char(&mut self, ch: char) {
        self.byte_pos += ch.len_utf8();
        self.char_index += 1;
        self.display_width += ch.width().unwrap_or(1);
    }

    fn set_to_end(&mut self, input: &str) {
        *self = Self::at_end(input);
    }
}

impl<'a> Readline<'a> {
    pub fn new(prompt: &'a str, history: &'a [HistoryEntry]) -> Self {
        Self { prompt, history }
    }

    pub fn read_line(&self) -> Result<ReadlineResult, ReadlineError> {
        enable_raw_mode()?;

        let mut stdout = io::stdout();
        let mut current_input = String::new();
        let mut cursor = CursorState::new(&current_input);
        let mut history_index: Option<usize> = None;
        let mut saved_input = String::new();
        let mut from_history_id: Option<String> = None;
        let mut modified = false;

        let result = (|| -> Result<ReadlineResult, ReadlineError> {
            render_line(
                &mut stdout,
                self.prompt,
                &current_input,
                cursor,
                from_history_id.is_some() && !modified,
            )?;

            loop {
                let event = read()?;
                let Event::Key(key_event) = event else {
                    continue;
                };

                match key_event.code {
                    KeyCode::Enter => {
                        writeln!(stdout)?;
                        return Ok(ReadlineResult {
                            line: current_input,
                            history_entry_id: from_history_id,
                            modified,
                        });
                    }
                    KeyCode::Char(c) => {
                        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                            if c.eq_ignore_ascii_case(&'c') {
                                let _ = writeln!(stdout);
                                return Err(ReadlineError::Cancelled);
                            }
                            continue;
                        }

                        current_input.insert(cursor.to_byte_pos(), c);
                        cursor.advance_by_char(c);
                        let _ = mark_modified_if_recalled(&from_history_id, &mut modified);
                        render_line(
                            &mut stdout,
                            self.prompt,
                            &current_input,
                            cursor,
                            from_history_id.is_some() && !modified,
                        )?;
                    }
                    KeyCode::Backspace => {
                        let state_changed =
                            mark_modified_if_recalled(&from_history_id, &mut modified);
                        let mut text_changed = false;
                        if cursor.to_byte_pos() > 0 {
                            let current_byte_pos = cursor.to_byte_pos();
                            if cursor.move_left(&current_input) {
                                current_input.drain(cursor.to_byte_pos()..current_byte_pos);
                                text_changed = true;
                            }
                        }

                        if text_changed || state_changed {
                            render_line(
                                &mut stdout,
                                self.prompt,
                                &current_input,
                                cursor,
                                from_history_id.is_some() && !modified,
                            )?;
                        }
                    }
                    KeyCode::Delete => {
                        let state_changed =
                            mark_modified_if_recalled(&from_history_id, &mut modified);
                        let mut text_changed = false;
                        if cursor.to_byte_pos() < current_input.len() {
                            let next = next_char_boundary(&current_input, cursor.to_byte_pos());
                            current_input.drain(cursor.to_byte_pos()..next);
                            text_changed = true;
                        }

                        if text_changed || state_changed {
                            render_line(
                                &mut stdout,
                                self.prompt,
                                &current_input,
                                cursor,
                                from_history_id.is_some() && !modified,
                            )?;
                        }
                    }
                    KeyCode::Left => {
                        if cursor.to_byte_pos() > 0 {
                            let old_width = cursor.display_width();
                            if cursor.move_left(&current_input) {
                                let new_width = cursor.display_width();
                                let columns_to_move =
                                    old_width.saturating_sub(new_width).min(u16::MAX as usize)
                                        as u16;
                                if columns_to_move > 0 {
                                    queue!(stdout, MoveLeft(columns_to_move))?;
                                    stdout.flush()?;
                                }
                            }
                        }
                    }
                    KeyCode::Right => {
                        if cursor.to_byte_pos() < current_input.len() {
                            let old_width = cursor.display_width();
                            if cursor.move_right(&current_input) {
                                let new_width = cursor.display_width();
                                let columns_to_move =
                                    new_width.saturating_sub(old_width).min(u16::MAX as usize)
                                        as u16;
                                if columns_to_move > 0 {
                                    queue!(stdout, MoveRight(columns_to_move))?;
                                    stdout.flush()?;
                                }
                            }
                        }
                    }
                    KeyCode::Up => {
                        if load_previous_history(
                            self.history,
                            &mut current_input,
                            &mut cursor,
                            &mut history_index,
                            &mut saved_input,
                            &mut from_history_id,
                            &mut modified,
                        ) {
                            render_line(
                                &mut stdout,
                                self.prompt,
                                &current_input,
                                cursor,
                                from_history_id.is_some() && !modified,
                            )?;
                        }
                    }
                    KeyCode::Down => {
                        if load_next_history(
                            self.history,
                            &mut current_input,
                            &mut cursor,
                            &mut history_index,
                            &saved_input,
                            &mut from_history_id,
                            &mut modified,
                        ) {
                            render_line(
                                &mut stdout,
                                self.prompt,
                                &current_input,
                                cursor,
                                from_history_id.is_some() && !modified,
                            )?;
                        }
                    }
                    KeyCode::Esc => {
                        let _ = writeln!(stdout);
                        return Err(ReadlineError::Cancelled);
                    }
                    _ => {}
                }
            }
        })();

        let disable_result = disable_raw_mode();
        match (result, disable_result) {
            (Ok(line), Ok(())) => Ok(line),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(ReadlineError::IoError(error)),
            (Err(error), Err(_)) => Err(error),
        }
    }
}

fn render_line(
    stdout: &mut impl Write,
    prompt: &str,
    input: &str,
    cursor: CursorState,
    is_recalled: bool,
) -> Result<(), ReadlineError> {
    queue!(
        stdout,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        Print(prompt),
    )?;

    if is_recalled {
        queue!(stdout, SetForegroundColor(Color::Cyan))?;
    }

    queue!(stdout, Print(input))?;

    if is_recalled {
        queue!(stdout, ResetColor)?;
    }

    let prompt_width = prompt.width();
    let cursor_width = cursor.display_width();
    let target_column = prompt_width
        .saturating_add(cursor_width)
        .min(u16::MAX as usize) as u16;

    queue!(stdout, MoveToColumn(target_column))?;
    stdout.flush()?;
    Ok(())
}

fn load_previous_history(
    history: &[HistoryEntry],
    current_input: &mut String,
    cursor: &mut CursorState,
    history_index: &mut Option<usize>,
    saved_input: &mut String,
    from_history_id: &mut Option<String>,
    modified: &mut bool,
) -> bool {
    if history.is_empty() {
        return false;
    }

    let next_index = match *history_index {
        None => {
            *saved_input = current_input.clone();
            history.len() - 1
        }
        Some(0) => 0,
        Some(index) => index.saturating_sub(1),
    };

    if *history_index == Some(next_index) {
        return false;
    }

    *history_index = Some(next_index);
    *current_input = history[next_index].prompt.clone();
    cursor.set_to_end(current_input);
    *from_history_id = Some(history[next_index].id.clone());
    *modified = false;
    true
}

fn load_next_history(
    history: &[HistoryEntry],
    current_input: &mut String,
    cursor: &mut CursorState,
    history_index: &mut Option<usize>,
    saved_input: &str,
    from_history_id: &mut Option<String>,
    modified: &mut bool,
) -> bool {
    if history.is_empty() {
        return false;
    }

    let Some(current_index) = *history_index else {
        return false;
    };

    if current_index + 1 < history.len() {
        let next_index = current_index + 1;
        *history_index = Some(next_index);
        *current_input = history[next_index].prompt.clone();
        *from_history_id = Some(history[next_index].id.clone());
    } else {
        *history_index = None;
        *current_input = saved_input.to_owned();
        *from_history_id = None;
    }

    cursor.set_to_end(current_input);
    *modified = false;
    true
}

fn mark_modified_if_recalled(from_history_id: &Option<String>, modified: &mut bool) -> bool {
    if from_history_id.is_some() && !*modified {
        *modified = true;
        true
    } else {
        false
    }
}

fn previous_char_boundary(input: &str, cursor_pos: usize) -> usize {
    if cursor_pos == 0 {
        return 0;
    }

    input
        .get(..cursor_pos)
        .and_then(|slice| slice.char_indices().last().map(|(idx, _)| idx))
        .unwrap_or(0)
}

fn next_char_boundary(input: &str, cursor_pos: usize) -> usize {
    if cursor_pos >= input.len() {
        return input.len();
    }

    input
        .get(cursor_pos..)
        .and_then(|slice| slice.chars().next().map(|ch| cursor_pos + ch.len_utf8()))
        .unwrap_or(input.len())
}

#[derive(Debug)]
pub enum ReadlineError {
    Cancelled,
    IoError(io::Error),
}

impl From<io::Error> for ReadlineError {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::history::HistoryEntry;

    use super::*;

    fn history_entry(id: &str, prompt: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.to_owned(),
            prompt: prompt.to_owned(),
            response: String::new(),
            timestamp: Utc::now(),
            provider_name: String::new(),
            model: String::new(),
        }
    }

    #[test]
    fn history_up_loads_latest_entry_first() {
        let history = vec![history_entry("1", "first"), history_entry("2", "second")];
        let mut input = "draft".to_owned();
        let mut cursor = CursorState::at_end(&input);
        let mut index = None;
        let mut saved = String::new();
        let mut from_history_id = None;
        let mut modified = true;

        let changed = load_previous_history(
            &history,
            &mut input,
            &mut cursor,
            &mut index,
            &mut saved,
            &mut from_history_id,
            &mut modified,
        );

        assert!(changed);
        assert_eq!(input, "second");
        assert_eq!(saved, "draft");
        assert_eq!(index, Some(1));
        assert_eq!(from_history_id.as_deref(), Some("2"));
        assert_eq!(cursor, CursorState::at_end(&input));
        assert!(!modified);
    }

    #[test]
    fn history_down_restores_saved_input_after_latest_entry() {
        let history = vec![history_entry("1", "first"), history_entry("2", "second")];
        let mut input = "second".to_owned();
        let mut cursor = CursorState::at_end(&input);
        let mut index = Some(1);
        let mut from_history_id = Some("2".to_owned());
        let mut modified = true;

        let changed = load_next_history(
            &history,
            &mut input,
            &mut cursor,
            &mut index,
            "draft",
            &mut from_history_id,
            &mut modified,
        );

        assert!(changed);
        assert_eq!(input, "draft");
        assert_eq!(index, None);
        assert!(from_history_id.is_none());
        assert_eq!(cursor, CursorState::at_end(&input));
        assert!(!modified);
    }

    #[test]
    fn cursor_state_rejects_non_boundary_positions() {
        assert_eq!(
            CursorState::from_byte_pos("가", "가".len()),
            Some(CursorState {
                byte_pos: "가".len(),
                char_index: 1,
                display_width: 2,
            })
        );
        assert!(CursorState::from_byte_pos("가", 1).is_none());
    }

    #[test]
    fn cursor_state_tracks_multibyte_movement() {
        let input = "가b";
        let mut cursor = CursorState::new(input);

        assert_eq!(cursor.to_byte_pos(), 0);
        assert_eq!(cursor.char_index(), 0);
        assert_eq!(cursor.display_width(), 0);

        assert!(cursor.move_right(input));
        assert_eq!(cursor.to_byte_pos(), "가".len());
        assert_eq!(cursor.char_index(), 1);
        assert_eq!(cursor.display_width(), 2);

        assert!(cursor.move_right(input));
        assert_eq!(cursor.to_byte_pos(), input.len());
        assert_eq!(cursor.char_index(), 2);
        assert_eq!(cursor.display_width(), 3);

        assert!(cursor.move_left(input));
        assert_eq!(cursor.to_byte_pos(), "가".len());
        assert_eq!(cursor.char_index(), 1);
        assert_eq!(cursor.display_width(), 2);
    }

    #[test]
    fn char_boundaries_follow_utf8_boundaries() {
        let input = "a\u{1F60A}b";
        let end = input.len();

        let first_left = previous_char_boundary(input, end);
        let second_left = previous_char_boundary(input, first_left);
        let third_left = previous_char_boundary(input, second_left);

        assert_eq!(first_left, "a😊".len());
        assert_eq!(second_left, "a".len());
        assert_eq!(third_left, 0);

        assert_eq!(next_char_boundary(input, 0), "a".len());
        assert_eq!(next_char_boundary(input, "a".len()), "a😊".len());
        assert_eq!(next_char_boundary(input, "a😊".len()), input.len());
    }
}
