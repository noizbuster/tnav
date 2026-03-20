use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    pub prompt: String,
    pub response: String,
    pub timestamp: DateTime<Utc>,
    pub provider_name: String,
    pub model: String,
}

impl HistoryEntry {
    pub fn new(prompt: String, response: String, provider_name: String, model: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            prompt,
            response,
            timestamp: Utc::now(),
            provider_name,
            model,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HistoryStore {
    pub profile: String,
    pub entries: Vec<HistoryEntry>,
}

impl HistoryStore {
    pub fn push(&mut self, entry: HistoryEntry) {
        if self
            .entries
            .first()
            .is_some_and(|existing| existing.prompt == entry.prompt)
        {
            self.entries.remove(0);
        }

        self.entries.insert(0, entry);

        if self.entries.len() > HISTORY_LIMIT {
            self.entries.truncate(HISTORY_LIMIT);
        }
    }

    pub fn find(&self, id: &str) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn recent(&self, limit: usize) -> &[HistoryEntry] {
        let end = limit.min(self.entries.len());
        &self.entries[..end]
    }
}
