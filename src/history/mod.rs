mod model;
mod store;

pub use model::{HistoryEntry, HistoryStore};
pub use store::{HistoryError, append_entry, history_file_path, load_history, save_history};
