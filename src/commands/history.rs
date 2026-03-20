use crate::cli::{GlobalArgs, HistoryArgs};
use crate::commands::executor::execute_command;
use crate::errors::TnavError;
use crate::history::{HistoryEntry, HistoryStore, load_history, save_history};
use crate::output::Output;
use crate::ui::{
    ConfirmResult, InquirePromptService, PromptOption, PromptService, confirm_execute_command,
    edit_command,
};

use super::map_prompt_error;

const PREVIEW_CLEAR_HEADROOM_LINES: usize = 2;

pub async fn run(global: &GlobalArgs, args: &HistoryArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let profile = resolve_profile_name(global);

    tracing::debug!(
        profile = %profile,
        limit = args.limit,
        clear = args.clear,
        json = args.json,
        global_json = output.is_json(),
        "Running history command"
    );

    if args.clear {
        return clear_history(&profile, &output);
    }

    if args.limit == 0 {
        return Err(TnavError::InvalidInput {
            message: "--limit must be greater than 0".to_owned(),
        });
    }

    let store = load_history(&profile)?;
    tracing::debug!(
        profile = %profile,
        entries = store.entries.len(),
        "Loaded history store"
    );

    if args.json || output.is_json() {
        output.print_json(&store)?;
        return Ok(());
    }

    if store.entries.is_empty() {
        output.line("No history entries for this profile.");
        output.line("");
        output.line("Run some commands first, then use 'tnav history' to recall them.");
        return Ok(());
    }

    let mut prompts = InquirePromptService::new();
    let selected = select_history_entry(&mut prompts, &store.entries, args.limit)?;

    if let Some(entry) = selected {
        tracing::debug!(
            profile = %profile,
            entry_id = %entry.id,
            "History entry selected"
        );

        display_entry_details(&output, &entry);

        let mut command = entry.response;
        let mut rendered_preview_lines: usize;

        loop {
            rendered_preview_lines = output.command_preview(&command);

            match confirm_execute_command(&mut prompts, &command).map_err(map_prompt_error)? {
                ConfirmResult::Execute => {
                    output.clear_rendered_lines(
                        rendered_preview_lines + PREVIEW_CLEAR_HEADROOM_LINES,
                    );
                    return execute_command(&command);
                }
                ConfirmResult::Edit => {
                    output.clear_rendered_lines(
                        rendered_preview_lines + PREVIEW_CLEAR_HEADROOM_LINES,
                    );
                    command = edit_command(&mut prompts, &command).map_err(map_prompt_error)?;
                }
                ConfirmResult::Cancel => return Err(TnavError::UserCancelled),
            }
        }
    }

    Ok(())
}

fn resolve_profile_name(global: &GlobalArgs) -> String {
    global
        .profile
        .clone()
        .unwrap_or_else(|| "default".to_string())
}

fn clear_history(profile: &str, output: &Output) -> Result<(), TnavError> {
    let empty_store = HistoryStore {
        profile: profile.to_string(),
        entries: Vec::new(),
    };

    save_history(&empty_store)?;

    tracing::debug!(profile = %profile, "Cleared history store");
    output.line(format!("History cleared for profile '{}'.", profile));

    Ok(())
}

fn select_history_entry(
    prompts: &mut impl PromptService,
    entries: &[HistoryEntry],
    limit: usize,
) -> Result<Option<HistoryEntry>, TnavError> {
    let display_entries: Vec<_> = entries.iter().take(limit).collect();

    if display_entries.is_empty() {
        return Ok(None);
    }

    let mut options: Vec<PromptOption> = display_entries
        .iter()
        .map(|entry| {
            let prompt_preview = truncate_string(&entry.prompt, 60);
            PromptOption::new(entry.id.clone(), prompt_preview)
        })
        .collect();

    options.push(PromptOption::new("cancel", "Cancel"));

    let selected = prompts
        .select_from_list("Select a history entry:", &options)
        .map_err(map_prompt_error)?;
    if selected == "cancel" {
        return Ok(None);
    }

    let selected_entry = display_entries
        .into_iter()
        .find(|entry| entry.id == selected)
        .cloned()
        .ok_or_else(|| TnavError::InvalidInput {
            message: format!("selected history entry '{selected}' does not exist"),
        })?;

    Ok(Some(selected_entry))
}

fn display_entry_details(output: &Output, entry: &HistoryEntry) {
    output.green_heading("History Entry");
    output.line(format!(
        "Time: {}",
        entry.timestamp.format("%Y-%m-%d %H:%M:%S")
    ));
    output.line(format!(
        "Provider: {} / {}",
        entry.provider_name, entry.model
    ));
    output.line("");
    output.yellow_heading("Prompt");
    output.line(format!("  {}", entry.prompt));
    output.line("");
    output.yellow_heading("Response");
}

fn truncate_string(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= max_len {
        return normalized;
    }

    if max_len <= 3 {
        return ".".repeat(max_len);
    }

    let truncated = normalized
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>();

    format!("{truncated}...")
}
