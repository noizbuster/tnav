pub fn redact_secret(value: &str) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();

    if char_count <= 6 {
        return "***".to_owned();
    }

    let prefix = trimmed.chars().take(3).collect::<String>();
    let suffix = trimmed
        .chars()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    format!("{prefix}...{suffix}")
}
