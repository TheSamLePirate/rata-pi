//! Transcript — ordered list of things that have happened in the session.
//!
//! Entries are render-agnostic; the view code in `app.rs` turns them into
//! Ratatui `Line`s. During streaming the last assistant entry grows in place.

use std::fmt;

#[derive(Debug, Clone)]
pub enum Entry {
    User(String),
    Assistant(String),
    Tool { name: String, status: ToolStatus },
    Info(String),
    Warn(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Ok,
    Err,
}

impl fmt::Display for ToolStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Running => "…",
            Self::Ok => "✓",
            Self::Err => "✗",
        })
    }
}

#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<Entry>,
}

impl Transcript {
    pub fn push(&mut self, e: Entry) {
        self.entries.push(e);
    }

    /// Append text to the last assistant entry, creating one if there's none
    /// or the latest entry isn't an assistant entry.
    pub fn append_assistant(&mut self, delta: &str) {
        match self.entries.last_mut() {
            Some(Entry::Assistant(s)) => s.push_str(delta),
            _ => self.entries.push(Entry::Assistant(delta.to_string())),
        }
    }

    /// Mark the most recent running tool entry with the given name complete.
    pub fn finish_tool(&mut self, name: &str, ok: bool) {
        for e in self.entries.iter_mut().rev() {
            if let Entry::Tool { name: n, status } = e
                && n == name
                && *status == ToolStatus::Running
            {
                *status = if ok { ToolStatus::Ok } else { ToolStatus::Err };
                return;
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
