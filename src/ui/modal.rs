//! Modal system — overlays that temporarily steal keyboard focus.
//!
//! The app holds at most one `Modal` at a time. When present it paints a
//! centered, bordered frame over the transcript area and routes input to
//! modal-specific handlers. `Modal::handle_key` returns a `ModalOutcome`
//! instructing the app whether to close, keep, or fire an RPC as the result.

use ratatui::layout::Rect;

use crate::rpc::types::{CommandInfo, Model, SessionStats, ThinkingLevel};

#[derive(Debug)]
pub enum Modal {
    Stats(Box<SessionStats>),
    Commands(ListModal<CommandInfo>),
    Models(ListModal<Model>),
    Thinking(RadioModal<ThinkingLevel>),
    Help,
}

/// A scrollable, filterable list of items.
#[derive(Debug)]
pub struct ListModal<T> {
    pub title: String,
    pub hint: String,
    pub query: String,
    pub items: Vec<T>,
    pub selected: usize,
}

impl<T> ListModal<T> {
    pub fn new(title: impl Into<String>, hint: impl Into<String>, items: Vec<T>) -> Self {
        Self {
            title: title.into(),
            hint: hint.into(),
            query: String::new(),
            items,
            selected: 0,
        }
    }
}

/// Radio-button modal for a fixed enum set.
#[derive(Debug)]
pub struct RadioModal<T> {
    pub title: String,
    pub options: Vec<(T, &'static str)>,
    pub selected: usize,
}

impl<T> RadioModal<T> {
    pub fn new(title: impl Into<String>, options: Vec<(T, &'static str)>, selected: usize) -> Self {
        Self {
            title: title.into(),
            options,
            selected,
        }
    }
}

/// Returns a rect centered in `area`, capped at `max_w x max_h` but never
/// exceeding 90 % of either dimension. For tiny terminals we just use what
/// fits.
pub fn centered(area: Rect, max_w: u16, max_h: u16) -> Rect {
    let w = max_w.min(area.width.saturating_mul(9) / 10).min(area.width);
    let h = max_h
        .min(area.height.saturating_mul(9) / 10)
        .min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Case-insensitive substring match for filter queries.
pub fn matches_query(haystack: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&query.to_lowercase())
}
