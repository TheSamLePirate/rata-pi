//! Interview mode — agent-authored structured forms.
//!
//! The agent asks the user for structured input by emitting a marker in
//! assistant text:
//!
//! ```text
//! [[INTERVIEW: { ...json interview definition... }]]
//! ```
//!
//! rata-pi parses the JSON on `agent_end`, opens a full-screen Interview
//! modal with sections and mixed field types, and on submit sends a user
//! message containing the answers back to the agent as a JSON payload
//! wrapped in `<interview-response>` tags.
//!
//! # JSON schema (v1)
//!
//! ```json
//! {
//!   "title": "Project setup",
//!   "description": "Let's scaffold a new app.",
//!   "submitLabel": "Create",
//!   "fields": [
//!     { "type": "section", "title": "Basics" },
//!     { "type": "text", "id": "name", "label": "Project name", "required": true,
//!       "placeholder": "my-app" },
//!     { "type": "select", "id": "framework", "label": "Framework",
//!       "options": ["React", "Vue", "Svelte", "None"], "default": "Vue" },
//!
//!     { "type": "section", "title": "Options" },
//!     { "type": "toggle", "id": "typescript", "label": "Use TypeScript?",
//!       "default": true },
//!     { "type": "multiselect", "id": "features", "label": "Include features",
//!       "options": ["router", "store", "testing", "i18n"],
//!       "default": ["router", "testing"] },
//!     { "type": "number", "id": "port", "label": "Dev-server port",
//!       "min": 1024, "max": 65535, "default": 5173 },
//!
//!     { "type": "section", "title": "Extra" },
//!     { "type": "text", "id": "notes", "label": "Additional notes",
//!       "multiline": true }
//!   ]
//! }
//! ```
//!
//! # Design notes
//!
//! * `section` and `info` are pseudo-fields: non-interactive, used for
//!   grouping / guidance. Real fields must have `id` + `label`.
//! * Field kinds: `text` / `toggle` / `select` / `multiselect` / `number` /
//!   `section` / `info`.
//! * Defaults hydrate the initial state; the user can override everything.
//! * Required fields are marked with `*` and block submission if empty.
//! * Responses serialize as JSON inside `<interview-response>` tags so
//!   the agent has a deterministic parse target.

use serde::Deserialize;

/// Top-level interview. Parsed from the `[[INTERVIEW: ...]]` marker.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interview {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub submit_label: Option<String>,
    pub fields: Vec<Field>,
}

/// A single form field. Sections and info rows are non-interactive.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Field {
    Section {
        title: String,
        #[serde(default)]
        description: Option<String>,
    },
    Info {
        text: String,
    },
    Text {
        id: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        default: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        multiline: bool,
    },
    Toggle {
        id: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: bool,
    },
    Select {
        id: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        options: Vec<String>,
        #[serde(default)]
        default: Option<String>,
        #[serde(default)]
        required: bool,
    },
    #[serde(alias = "checkboxes")]
    Multiselect {
        id: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        options: Vec<String>,
        #[serde(default)]
        default: Vec<String>,
    },
    Number {
        id: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        default: Option<f64>,
        #[serde(default)]
        required: bool,
    },
}

// `Field` is the serde-parsed input; once hydrated into `FieldValue` the
// parser copies own these getters but the tests still verify the Field
// helpers work, so keep them wrapped in #[allow(dead_code)] rather than
// deleting.
#[allow(dead_code)]
impl Field {
    /// True when this field stores a user-supplied value (and counts for
    /// tab navigation / required-validation).
    pub fn is_interactive(&self) -> bool {
        !matches!(self, Field::Section { .. } | Field::Info { .. })
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Field::Section { .. } | Field::Info { .. } => None,
            Field::Text { id, .. }
            | Field::Toggle { id, .. }
            | Field::Select { id, .. }
            | Field::Multiselect { id, .. }
            | Field::Number { id, .. } => Some(id),
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Field::Section { title, .. } => Some(title),
            Field::Info { .. } => None,
            Field::Text { label, .. }
            | Field::Toggle { label, .. }
            | Field::Select { label, .. }
            | Field::Multiselect { label, .. }
            | Field::Number { label, .. } => Some(label),
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            Field::Section { description, .. }
            | Field::Text { description, .. }
            | Field::Toggle { description, .. }
            | Field::Select { description, .. }
            | Field::Multiselect { description, .. }
            | Field::Number { description, .. } => description.as_deref(),
            Field::Info { .. } => None,
        }
    }

    pub fn required(&self) -> bool {
        match self {
            Field::Text { required, .. }
            | Field::Select { required, .. }
            | Field::Number { required, .. } => *required,
            _ => false,
        }
    }
}

// ───────────────────────────────────────────────────────────── state ──

/// Live user-answered state for a field. Mirrors `Field` kinds with
/// mutable value slots. Non-interactive kinds don't need a value slot —
/// we keep `Section` / `Info` to preserve order and render them.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Section {
        title: String,
        description: Option<String>,
    },
    Info {
        text: String,
    },
    Text {
        id: String,
        label: String,
        description: Option<String>,
        placeholder: Option<String>,
        value: String,
        /// Byte-offset cursor into `value`.
        cursor: usize,
        required: bool,
        multiline: bool,
    },
    Toggle {
        id: String,
        label: String,
        description: Option<String>,
        value: bool,
    },
    Select {
        id: String,
        label: String,
        description: Option<String>,
        options: Vec<String>,
        selected: usize,
        required: bool,
    },
    Multiselect {
        id: String,
        label: String,
        description: Option<String>,
        options: Vec<String>,
        checked: Vec<bool>,
        /// Horizontal cursor inside the option row.
        cursor: usize,
    },
    Number {
        id: String,
        label: String,
        description: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
        value: String,
        cursor: usize,
        required: bool,
    },
}

impl FieldValue {
    pub fn is_interactive(&self) -> bool {
        !matches!(self, Self::Section { .. } | Self::Info { .. })
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Section { title, .. } => Some(title),
            Self::Info { .. } => None,
            Self::Text { label, .. }
            | Self::Toggle { label, .. }
            | Self::Select { label, .. }
            | Self::Multiselect { label, .. }
            | Self::Number { label, .. } => Some(label),
        }
    }

    #[allow(dead_code)] // exposed for future UI hooks (tooltips, validation reports)
    pub fn description(&self) -> Option<&str> {
        match self {
            Self::Section { description, .. }
            | Self::Text { description, .. }
            | Self::Toggle { description, .. }
            | Self::Select { description, .. }
            | Self::Multiselect { description, .. }
            | Self::Number { description, .. } => description.as_deref(),
            Self::Info { .. } => None,
        }
    }

    #[allow(dead_code)] // consumed via `missing_required`; kept for symmetry with Field
    pub fn required(&self) -> bool {
        match self {
            Self::Text { required, .. }
            | Self::Select { required, .. }
            | Self::Number { required, .. } => *required,
            _ => false,
        }
    }

    /// Is this field missing a required answer? Used to block submit.
    pub fn missing_required(&self) -> bool {
        match self {
            Self::Text {
                required, value, ..
            } if *required => value.trim().is_empty(),
            Self::Number {
                required, value, ..
            } if *required => value.trim().is_empty(),
            Self::Select { required, .. } if *required => false, // always has a pick
            _ => false,
        }
    }
}

/// Live state of an interview in progress — what the modal renders and
/// mutates on keystrokes.
#[derive(Debug, Clone)]
pub struct InterviewState {
    pub title: String,
    pub description: Option<String>,
    pub submit_label: String,
    pub fields: Vec<FieldValue>,
    /// Index into `fields` of the currently focused entry. Navigation
    /// skips non-interactive kinds.
    pub focus: usize,
    /// Vertical scroll offset for tall forms. Not wired into the draw
    /// path yet — the modal frame's natural clip handles typical forms.
    #[allow(dead_code)]
    pub scroll: u16,
    /// When set, `submit` was attempted but a required field was empty.
    pub validation_error: Option<String>,
}

impl InterviewState {
    /// Hydrate a fresh state from a parsed Interview definition (defaults
    /// applied, focus pinned at the first interactive field).
    pub fn from_interview(iv: Interview) -> Self {
        let fields: Vec<FieldValue> = iv
            .fields
            .into_iter()
            .map(|f| match f {
                Field::Section { title, description } => FieldValue::Section { title, description },
                Field::Info { text } => FieldValue::Info { text },
                Field::Text {
                    id,
                    label,
                    description,
                    placeholder,
                    default,
                    required,
                    multiline,
                } => {
                    let value = default.unwrap_or_default();
                    let cursor = value.len();
                    FieldValue::Text {
                        id,
                        label,
                        description,
                        placeholder,
                        value,
                        cursor,
                        required,
                        multiline,
                    }
                }
                Field::Toggle {
                    id,
                    label,
                    description,
                    default,
                } => FieldValue::Toggle {
                    id,
                    label,
                    description,
                    value: default,
                },
                Field::Select {
                    id,
                    label,
                    description,
                    options,
                    default,
                    required,
                } => {
                    let selected = default
                        .as_deref()
                        .and_then(|d| options.iter().position(|o| o == d))
                        .unwrap_or(0);
                    FieldValue::Select {
                        id,
                        label,
                        description,
                        options,
                        selected,
                        required,
                    }
                }
                Field::Multiselect {
                    id,
                    label,
                    description,
                    options,
                    default,
                } => {
                    let checked: Vec<bool> = options
                        .iter()
                        .map(|o| default.iter().any(|d| d == o))
                        .collect();
                    FieldValue::Multiselect {
                        id,
                        label,
                        description,
                        options,
                        checked,
                        cursor: 0,
                    }
                }
                Field::Number {
                    id,
                    label,
                    description,
                    min,
                    max,
                    default,
                    required,
                } => {
                    let value = default.map(format_number).unwrap_or_default();
                    let cursor = value.len();
                    FieldValue::Number {
                        id,
                        label,
                        description,
                        min,
                        max,
                        value,
                        cursor,
                        required,
                    }
                }
            })
            .collect();
        let focus = fields.iter().position(|f| f.is_interactive()).unwrap_or(0);
        Self {
            title: iv.title,
            description: iv.description,
            submit_label: iv.submit_label.unwrap_or_else(|| "Submit".to_string()),
            fields,
            focus,
            scroll: 0,
            validation_error: None,
        }
    }

    /// Index of the virtual "submit button" focus slot. One past the last
    /// field. Navigation cycles through interactive fields AND this slot,
    /// so the user can Tab through to an explicit Submit position and
    /// press Enter to send.
    pub fn submit_slot(&self) -> usize {
        self.fields.len()
    }

    /// True when focus is currently on the submit slot (not on a field).
    pub fn focus_on_submit(&self) -> bool {
        self.focus == self.submit_slot()
    }

    /// Advance focus to the next interactive field OR the submit slot.
    /// Wraps around to the first interactive field.
    pub fn focus_next(&mut self) {
        let n = self.fields.len();
        if n == 0 {
            return;
        }
        let total = n + 1; // +1 for the submit slot
        for i in 1..=total {
            let cand = (self.focus + i) % total;
            if cand == n || self.fields[cand].is_interactive() {
                self.focus = cand;
                return;
            }
        }
    }

    /// Move focus to the previous interactive field or the submit slot.
    pub fn focus_prev(&mut self) {
        let n = self.fields.len();
        if n == 0 {
            return;
        }
        let total = n + 1;
        for i in 1..=total {
            let cand = (self.focus + total - i) % total;
            if cand == n || self.fields[cand].is_interactive() {
                self.focus = cand;
                return;
            }
        }
    }

    /// Validate required fields. Returns the label of the first missing
    /// required field (for the flash toast / modal footer), or None.
    pub fn first_missing_required(&self) -> Option<&str> {
        for f in &self.fields {
            if f.missing_required() {
                return f.label();
            }
        }
        None
    }

    /// Serialise the filled-in state as the structured JSON payload the
    /// agent receives. Wrapped in `<interview-response>` tags so it's
    /// easy to locate inside a larger user message.
    pub fn as_response(&self) -> String {
        let mut map = serde_json::Map::new();
        for f in &self.fields {
            match f {
                FieldValue::Section { .. } | FieldValue::Info { .. } => continue,
                FieldValue::Text { id, value, .. } => {
                    map.insert(id.clone(), serde_json::Value::String(value.clone()));
                }
                FieldValue::Toggle { id, value, .. } => {
                    map.insert(id.clone(), serde_json::Value::Bool(*value));
                }
                FieldValue::Select {
                    id,
                    options,
                    selected,
                    ..
                } => {
                    let val = options.get(*selected).cloned().unwrap_or_default();
                    map.insert(id.clone(), serde_json::Value::String(val));
                }
                FieldValue::Multiselect {
                    id,
                    options,
                    checked,
                    ..
                } => {
                    let vals: Vec<serde_json::Value> = options
                        .iter()
                        .zip(checked.iter())
                        .filter_map(|(o, &c)| {
                            if c {
                                Some(serde_json::Value::String(o.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    map.insert(id.clone(), serde_json::Value::Array(vals));
                }
                FieldValue::Number { id, value, .. } => {
                    let parsed = value
                        .parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null);
                    map.insert(id.clone(), parsed);
                }
            }
        }
        let payload = serde_json::json!({
            "title": self.title,
            "answers": map,
        });
        let pretty = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
        format!("<interview-response>\n{pretty}\n</interview-response>")
    }

    /// A short plain-text preview ("name=my-app · features=router,testing")
    /// used for the transcript card that records the submission.
    pub fn human_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for f in &self.fields {
            match f {
                FieldValue::Text { id, value, .. } if !value.is_empty() => {
                    parts.push(format!("{id}={value}"));
                }
                FieldValue::Toggle { id, value, .. } => {
                    parts.push(format!("{id}={}", if *value { "yes" } else { "no" }));
                }
                FieldValue::Select {
                    id,
                    options,
                    selected,
                    ..
                } => {
                    let v = options.get(*selected).cloned().unwrap_or_default();
                    parts.push(format!("{id}={v}"));
                }
                FieldValue::Multiselect {
                    id,
                    options,
                    checked,
                    ..
                } => {
                    let list: Vec<&str> = options
                        .iter()
                        .zip(checked.iter())
                        .filter_map(|(o, &c)| if c { Some(o.as_str()) } else { None })
                        .collect();
                    parts.push(format!("{id}=[{}]", list.join(",")));
                }
                FieldValue::Number { id, value, .. } if !value.is_empty() => {
                    parts.push(format!("{id}={value}"));
                }
                _ => {}
            }
        }
        parts.join(" · ")
    }
}

fn format_number(n: f64) -> String {
    // Keep integers crisp when the value is whole.
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{:.0}", n)
    } else {
        format!("{n}")
    }
}

// ─────────────────────────────────────── flat-marker grammar (primary) ──
//
// Mirrors plan-mode's marker style. One marker per line, pipe-separated
// fields. Models in the wild emit these FAR more reliably than nested
// JSON. If any `[[ASK_*]]` marker appears in assistant text, we collect
// all of them (in document order) and build a single `Interview`.

/// One parsed `[[ASK_*]]` marker plus the byte-range it occupied in the
/// source text (so the caller can strip every marker from the display).
#[derive(Debug, Clone)]
struct ParsedAsk {
    kind: AskKind,
    range: std::ops::Range<usize>,
}

#[derive(Debug, Clone)]
enum AskKind {
    Title(String),
    Desc(String),
    Section(String),
    Info(String),
    Submit(String),
    Text {
        id: String,
        label: String,
        default: Option<String>,
        required: bool,
        multiline: bool,
    },
    Yesno {
        id: String,
        label: String,
        default: bool,
    },
    Num {
        id: String,
        label: String,
        default: Option<f64>,
        min: Option<f64>,
        max: Option<f64>,
        required: bool,
    },
    Pick {
        id: String,
        label: String,
        options: Vec<String>,
        default: Option<String>,
        required: bool,
    },
    Multi {
        id: String,
        label: String,
        options: Vec<String>,
        default: Vec<String>,
    },
}

fn scan_ask_markers(text: &str) -> Vec<ParsedAsk> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = text[i..].find("[[ASK_") {
        let start = i + rel;
        let after_open = start + 2; // after "[[", pointing at "ASK_…"
        let Some(close_rel) = text[after_open..].find("]]") else {
            break;
        };
        let body_end = after_open + close_rel;
        let end = body_end + 2;
        let body = &text[after_open..body_end];
        if let Some(kind) = parse_ask_body(body) {
            out.push(ParsedAsk {
                kind,
                range: start..end,
            });
        }
        i = end;
    }
    out
}

fn parse_ask_body(body: &str) -> Option<AskKind> {
    // Expected shape: `ASK_KIND[!]:<payload>`
    let colon = body.find(':')?;
    let head = body[..colon].trim();
    let payload = body[colon + 1..].trim();

    let (kind_name, required) = match head.strip_suffix('!') {
        Some(k) => (k.trim_end(), true),
        None => (head, false),
    };
    let kind_name = kind_name.strip_prefix("ASK_")?;

    match kind_name {
        "TITLE" => Some(AskKind::Title(payload.to_string())),
        "DESC" => Some(AskKind::Desc(payload.to_string())),
        "SECTION" => Some(AskKind::Section(payload.to_string())),
        "INFO" => Some(AskKind::Info(payload.to_string())),
        "SUBMIT" => Some(AskKind::Submit(payload.to_string())),

        "TEXT" | "AREA" => {
            let parts = pipe_split(payload);
            if parts.len() < 2 {
                return None;
            }
            Some(AskKind::Text {
                id: parts[0].clone(),
                label: parts[1].clone(),
                default: parts.get(2).cloned().filter(|s| !s.is_empty()),
                required,
                multiline: kind_name == "AREA",
            })
        }

        "YESNO" => {
            let parts = pipe_split(payload);
            if parts.len() < 2 {
                return None;
            }
            let default = parts
                .get(2)
                .map(|s| parse_bool(s).unwrap_or(false))
                .unwrap_or(false);
            Some(AskKind::Yesno {
                id: parts[0].clone(),
                label: parts[1].clone(),
                default,
            })
        }

        "NUM" => {
            let parts = pipe_split(payload);
            if parts.len() < 2 {
                return None;
            }
            let default = parts.get(2).and_then(|s| s.parse::<f64>().ok());
            let (min, max) = parts.get(3).map(|s| parse_range(s)).unwrap_or((None, None));
            Some(AskKind::Num {
                id: parts[0].clone(),
                label: parts[1].clone(),
                default,
                min,
                max,
                required,
            })
        }

        "PICK" => {
            let parts = pipe_split(payload);
            if parts.len() < 3 {
                return None;
            }
            let id = parts[0].clone();
            let label = parts[1].clone();
            let (options, default) = extract_options_and_default(&parts[2..]);
            Some(AskKind::Pick {
                id,
                label,
                options,
                default,
                required,
            })
        }

        "MULTI" => {
            let parts = pipe_split(payload);
            if parts.len() < 3 {
                return None;
            }
            let id = parts[0].clone();
            let label = parts[1].clone();
            let (options, defaults) = extract_options_and_multi_defaults(&parts[2..]);
            Some(AskKind::Multi {
                id,
                label,
                options,
                default: defaults,
            })
        }

        _ => None,
    }
}

fn pipe_split(s: &str) -> Vec<String> {
    s.split('|').map(|p| p.trim().to_string()).collect()
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "yes" | "y" | "true" | "on" | "1" => Some(true),
        "no" | "n" | "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_range(s: &str) -> (Option<f64>, Option<f64>) {
    // Accept "min-max", "min..max", "min,max". All parts optional.
    let sep = s
        .find("..")
        .map(|i| (i, 2))
        .or_else(|| s.find('-').map(|i| (i, 1)))
        .or_else(|| s.find(',').map(|i| (i, 1)));
    if let Some((i, len)) = sep {
        let lo = s[..i].trim();
        let hi = s[i + len..].trim();
        (lo.parse::<f64>().ok(), hi.parse::<f64>().ok())
    } else {
        (None, None)
    }
}

fn extract_options_and_default(parts: &[String]) -> (Vec<String>, Option<String>) {
    let mut options = Vec::new();
    let mut default = None;
    for raw in parts {
        if raw.is_empty() {
            continue;
        }
        if let Some(name) = raw.strip_suffix('*') {
            let name = name.trim().to_string();
            options.push(name.clone());
            default = Some(name);
        } else {
            options.push(raw.clone());
        }
    }
    (options, default)
}

fn extract_options_and_multi_defaults(parts: &[String]) -> (Vec<String>, Vec<String>) {
    let mut options = Vec::new();
    let mut defaults = Vec::new();
    for raw in parts {
        if raw.is_empty() {
            continue;
        }
        if let Some(name) = raw.strip_suffix('*') {
            let name = name.trim().to_string();
            options.push(name.clone());
            defaults.push(name);
        } else {
            options.push(raw.clone());
        }
    }
    (options, defaults)
}

/// Build an [`Interview`] from a run of ASK markers (in document order).
/// Returns None if the run has no interactive fields.
fn assemble_interview(asks: Vec<ParsedAsk>) -> Option<Interview> {
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut submit_label: Option<String> = None;
    let mut fields: Vec<Field> = Vec::new();
    let mut has_interactive = false;

    for a in asks {
        match a.kind {
            AskKind::Title(t) if title.is_none() => title = Some(t),
            AskKind::Desc(d) if description.is_none() => description = Some(d),
            AskKind::Submit(s) if submit_label.is_none() => submit_label = Some(s),
            AskKind::Title(_) | AskKind::Desc(_) | AskKind::Submit(_) => {}

            AskKind::Section(title) => {
                fields.push(Field::Section {
                    title,
                    description: None,
                });
            }
            AskKind::Info(text) => {
                fields.push(Field::Info { text });
            }
            AskKind::Text {
                id,
                label,
                default,
                required,
                multiline,
            } => {
                has_interactive = true;
                fields.push(Field::Text {
                    id,
                    label,
                    description: None,
                    placeholder: None,
                    default,
                    required,
                    multiline,
                });
            }
            AskKind::Yesno { id, label, default } => {
                has_interactive = true;
                fields.push(Field::Toggle {
                    id,
                    label,
                    description: None,
                    default,
                });
            }
            AskKind::Num {
                id,
                label,
                default,
                min,
                max,
                required,
            } => {
                has_interactive = true;
                fields.push(Field::Number {
                    id,
                    label,
                    description: None,
                    min,
                    max,
                    default,
                    required,
                });
            }
            AskKind::Pick {
                id,
                label,
                options,
                default,
                required,
            } => {
                has_interactive = true;
                fields.push(Field::Select {
                    id,
                    label,
                    description: None,
                    options,
                    default,
                    required,
                });
            }
            AskKind::Multi {
                id,
                label,
                options,
                default,
            } => {
                has_interactive = true;
                fields.push(Field::Multiselect {
                    id,
                    label,
                    description: None,
                    options,
                    default,
                });
            }
        }
    }

    if !has_interactive {
        return None;
    }
    Some(Interview {
        title: title.unwrap_or_else(|| "Questions".to_string()),
        description,
        submit_label,
        fields,
    })
}

/// Primary marker-based detector. Scans for `[[ASK_*]]` markers and,
/// if at least one interactive field is present, returns the assembled
/// interview plus the full list of byte-ranges to strip from display.
pub fn parse_ask_markers(text: &str) -> Option<(Interview, Vec<std::ops::Range<usize>>)> {
    let asks = scan_ask_markers(text);
    if asks.is_empty() {
        return None;
    }
    let ranges: Vec<std::ops::Range<usize>> = asks.iter().map(|a| a.range.clone()).collect();
    let iv = assemble_interview(asks)?;
    Some((iv, ranges))
}

// ─────────────────────────────────────────────────────────── parsing ──

const MARKER_OPEN: &str = "[[INTERVIEW:";

/// Scan `text` for a `[[INTERVIEW: …]]` marker and parse it. Returns the
/// first valid interview found, plus the byte-range of the marker in the
/// source string so callers can strip it from display.
pub fn parse_marker(text: &str) -> Option<(Interview, std::ops::Range<usize>)> {
    let start = text.find(MARKER_OPEN)?;
    // Balanced-brace scan: the JSON body is allowed to contain `]` inside
    // strings and nested arrays, so we can't just `find("]]")`. Walk
    // forward counting braces and quoted strings until we see `]]` at
    // nesting depth 0.
    let after_open = start + MARKER_OPEN.len();
    let bytes = text.as_bytes();
    let mut i = after_open;
    let mut depth_curly: i32 = 0;
    let mut depth_square: i32 = 0;
    let mut in_str = false;
    let mut escape = false;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth_curly += 1,
            b'}' => depth_curly -= 1,
            b'[' => depth_square += 1,
            b']' if bytes[i + 1] == b']' && depth_curly == 0 && depth_square == 0 => {
                // End of marker.
                let json = text[after_open..i].trim();
                let end = i + 2;
                let iv = serde_json::from_str::<Interview>(json).ok()?;
                return Some((iv, start..end));
            }
            b']' => depth_square -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Remove the marker from a string so the display text doesn't echo the
/// raw JSON at the user. Returns a clean copy.
#[allow(dead_code)]
pub fn strip_marker(text: &str) -> String {
    match parse_marker(text) {
        Some((_, range)) => strip_range(text, range),
        None => text.to_string(),
    }
}

/// Remove the byte-range `range` from `text` and return the remainder
/// with surrounding whitespace collapsed (so the assistant card doesn't
/// end up with a blank paragraph where the form used to live).
pub fn strip_range(text: &str, range: std::ops::Range<usize>) -> String {
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..range.start]);
    out.push_str(&text[range.end..]);
    // Collapse any run of 3+ newlines (common after a removed block) to 2.
    let mut collapsed = String::with_capacity(out.len());
    let mut newline_run = 0usize;
    for ch in out.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                collapsed.push(ch);
            }
        } else {
            newline_run = 0;
            collapsed.push(ch);
        }
    }
    collapsed.trim().to_string()
}

/// Robust interview detector: tries multiple shapes in priority order.
///
/// 1. **Flat ASK markers** (primary, mirrors plan-mode's reliable style):
///    a run of `[[ASK_TEXT: …]]`, `[[ASK_PICK: …]]`, etc. Each marker is
///    one line, pipe-separated fields, no JSON. Models emit these
///    correctly almost every time.
/// 2. Canonical `[[INTERVIEW: …json…]]` marker (advanced, for agents
///    that prefer a single structured payload).
/// 3. Any fenced code block whose body deserializes as an [`Interview`].
/// 4. A bare JSON object that deserializes the same way.
///
/// Returns the parsed Interview plus a list of byte-ranges to strip from
/// the visible transcript (so the user sees the agent's prose without
/// raw markers or JSON). The flat-marker path returns one range per
/// marker; the JSON paths return a single range.
pub fn detect_interview(text: &str) -> Option<(Interview, Vec<std::ops::Range<usize>>)> {
    // Primary: flat ASK markers, one per line.
    if let Some(hit) = parse_ask_markers(text) {
        return Some(hit);
    }
    // Fallback 1: JSON marker.
    if let Some((iv, range)) = parse_marker(text) {
        return Some((iv, vec![range]));
    }
    // Fallback 2: fenced JSON code block.
    if let Some((iv, range)) = scan_fenced_blocks(text) {
        return Some((iv, vec![range]));
    }
    // Fallback 3: bare JSON object.
    let (iv, range) = scan_bare_json(text)?;
    Some((iv, vec![range]))
}

/// Strip a set of byte-ranges (possibly overlapping) from `text` and
/// return the remainder with surrounding whitespace collapsed.
pub fn strip_ranges(text: &str, mut ranges: Vec<std::ops::Range<usize>>) -> String {
    // Sort by start, coalesce overlaps, then strip right-to-left so
    // earlier indices stay valid.
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
            continue;
        }
        merged.push(r);
    }
    let mut out = text.to_string();
    for r in merged.into_iter().rev() {
        let r = r.start.min(out.len())..r.end.min(out.len());
        out.replace_range(r, "");
    }
    // Collapse 3+ newlines to 2 (common after stripping a block of
    // markers separated by newlines).
    let mut collapsed = String::with_capacity(out.len());
    let mut newline_run = 0usize;
    for ch in out.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                collapsed.push(ch);
            }
        } else {
            newline_run = 0;
            collapsed.push(ch);
        }
    }
    collapsed.trim().to_string()
}

/// Find the first fenced code block (``` … ```) whose body parses as an
/// Interview. The language tag is ignored — only the content matters.
fn scan_fenced_blocks(text: &str) -> Option<(Interview, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"```" {
            let fence_start = i;
            let mut j = i + 3;
            // Skip language tag up to newline.
            while j < bytes.len() && bytes[j] != b'\n' {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            let body_start = j + 1;
            // Find closing fence on its own line.
            let Some(close_rel) = text[body_start..].find("```") else {
                break;
            };
            let body_end = body_start + close_rel;
            let fence_end = body_end + 3;
            let body = text[body_start..body_end].trim();
            if let Some(iv) = try_parse_interview(body) {
                return Some((iv, fence_start..fence_end));
            }
            i = fence_end;
        } else {
            i += 1;
        }
    }
    None
}

/// Find the first balanced `{…}` JSON object in `text` that parses as an
/// Interview.
fn scan_bare_json(text: &str) -> Option<(Interview, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Walk forward keeping balanced-brace state and tolerating
            // quoted strings, then try to parse the resulting slice.
            let mut depth: i32 = 0;
            let mut in_str = false;
            let mut escape = false;
            let start = i;
            let mut j = i;
            while j < bytes.len() {
                let b = bytes[j];
                if in_str {
                    if escape {
                        escape = false;
                    } else if b == b'\\' {
                        escape = true;
                    } else if b == b'"' {
                        in_str = false;
                    }
                    j += 1;
                    continue;
                }
                match b {
                    b'"' => in_str = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            let candidate = &text[start..=j];
                            if let Some(iv) = try_parse_interview(candidate) {
                                return Some((iv, start..j + 1));
                            }
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i = j.saturating_add(1);
        } else {
            i += 1;
        }
    }
    None
}

/// Parse a JSON slice as an [`Interview`] and apply the "not accidental"
/// validation rules — non-empty `title` + non-empty `fields` + at least
/// one recognized field type.
fn try_parse_interview(s: &str) -> Option<Interview> {
    let iv: Interview = serde_json::from_str(s).ok()?;
    if iv.title.trim().is_empty() {
        return None;
    }
    if iv.fields.is_empty() {
        return None;
    }
    // At least one interactive field (not just sections/info).
    if !iv.fields.iter().any(|f| f.is_interactive()) {
        return None;
    }
    Some(iv)
}

/// A short natural-language hint the host can inject into outgoing
/// prompts so the agent knows it can author an interview.
pub fn capability_hint() -> &'static str {
    concat!(
        "\n\n(rata-pi interview protocol — when you need several related ",
        "answers from the user, emit these one-per-line markers. Pipe (|) ",
        "separates fields; trailing `!` on the kind = required; trailing ",
        "`*` on an option = default / preselected. All markers are ",
        "stripped from your visible card, the modal opens, and the ",
        "user's answers come back as a <interview-response> JSON block.\n",
        "\n",
        "  [[ASK_TITLE: Project setup]]\n",
        "  [[ASK_DESC: Tell me how to scaffold this]]\n",
        "  [[ASK_SECTION: Basics]]\n",
        "  [[ASK_INFO: Any note or guidance]]\n",
        "  [[ASK_TEXT!: name | Project name | my-app]]        // required\n",
        "  [[ASK_AREA: notes | Additional notes]]             // multiline\n",
        "  [[ASK_YESNO: typescript | Use TypeScript? | yes]]\n",
        "  [[ASK_PICK: fw | Framework | React | Vue* | Svelte]] // * = default\n",
        "  [[ASK_MULTI: features | Features | router* | store | testing*]]\n",
        "  [[ASK_NUM!: port | Port | 5173 | 1024-65535]]\n",
        "  [[ASK_SUBMIT: Create]]                             // optional button label\n",
        "\n",
        "Prefer ONE interview over several free-form questions when the ",
        "questions are related.)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_interview() {
        let src = r#"Please fill this out: [[INTERVIEW: {
            "title": "Hi",
            "fields": [
                { "type": "text", "id": "name", "label": "Name" }
            ]
        }]] thanks!"#;
        let (iv, range) = parse_marker(src).expect("parses");
        assert_eq!(iv.title, "Hi");
        assert_eq!(iv.fields.len(), 1);
        assert!(
            matches!(&iv.fields[0], Field::Text { id, label, .. } if id == "name" && label == "Name")
        );
        // Range should exactly span the marker including brackets.
        assert!(src[range.start..range.end].starts_with("[[INTERVIEW:"));
        assert!(src[range.start..range.end].ends_with("]]"));
    }

    #[test]
    fn strips_marker_cleanly() {
        let src = "hello [[INTERVIEW: {\"title\":\"X\",\"fields\":[]}]] world";
        let out = strip_marker(src);
        assert!(!out.contains("INTERVIEW"));
        assert!(out.contains("hello"));
        assert!(out.contains("world"));
    }

    #[test]
    fn parses_all_field_kinds() {
        let src = r#"[[INTERVIEW: {
            "title": "All fields",
            "description": "Try every kind.",
            "submitLabel": "Go",
            "fields": [
                { "type": "section", "title": "Text stuff" },
                { "type": "info", "text": "Here's some guidance." },
                { "type": "text", "id": "t1", "label": "T1",
                  "placeholder": "eg", "default": "x", "required": true },
                { "type": "text", "id": "t2", "label": "T2", "multiline": true },
                { "type": "toggle", "id": "b", "label": "Bool", "default": true },
                { "type": "select", "id": "s", "label": "S",
                  "options": ["a","b","c"], "default": "b" },
                { "type": "multiselect", "id": "m", "label": "M",
                  "options": ["x","y","z"], "default": ["y"] },
                { "type": "checkboxes", "id": "cb", "label": "CB",
                  "options": ["p","q"] },
                { "type": "number", "id": "n", "label": "N",
                  "min": 0, "max": 10, "default": 3, "required": true }
            ]
        }]]"#;
        let (iv, _) = parse_marker(src).expect("parses");
        assert_eq!(iv.title, "All fields");
        assert_eq!(iv.description.as_deref(), Some("Try every kind."));
        assert_eq!(iv.submit_label.as_deref(), Some("Go"));
        assert_eq!(iv.fields.len(), 9);
        // Checkboxes alias resolves to Multiselect.
        assert!(matches!(&iv.fields[7], Field::Multiselect { id, .. } if id == "cb"));
    }

    #[test]
    fn tolerates_brackets_and_quotes_in_strings() {
        // Make sure the balanced-brace scan doesn't mis-terminate at a
        // `]]` sequence nested inside a JSON string value.
        let src = r#"[[INTERVIEW: {
            "title": "with [[brackets]] in title",
            "fields": [{
                "type": "text", "id": "x", "label": "Say \"hi\"",
                "default": "abc]]def"
            }]
        }]]"#;
        let (iv, _) = parse_marker(src).expect("parses");
        assert_eq!(iv.title, "with [[brackets]] in title");
        match &iv.fields[0] {
            Field::Text { label, default, .. } => {
                assert_eq!(label, "Say \"hi\"");
                assert_eq!(default.as_deref(), Some("abc]]def"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn returns_none_when_no_marker() {
        assert!(parse_marker("just a normal message").is_none());
    }

    #[test]
    fn returns_none_on_invalid_json() {
        // Marker is present but JSON is malformed.
        assert!(parse_marker("[[INTERVIEW: {not valid json}]]").is_none());
    }

    #[test]
    fn field_helpers() {
        let f = Field::Text {
            id: "n".into(),
            label: "Name".into(),
            description: Some("Your name".into()),
            placeholder: None,
            default: None,
            required: true,
            multiline: false,
        };
        assert!(f.is_interactive());
        assert_eq!(f.id(), Some("n"));
        assert_eq!(f.label(), Some("Name"));
        assert_eq!(f.description(), Some("Your name"));
        assert!(f.required());

        let section = Field::Section {
            title: "S".into(),
            description: None,
        };
        assert!(!section.is_interactive());
        assert_eq!(section.id(), None);
        assert_eq!(section.label(), Some("S"));
    }

    #[test]
    fn capability_hint_documents_ask_markers() {
        let h = capability_hint();
        assert!(h.contains("ASK_TEXT"));
        assert!(h.contains("ASK_PICK"));
        assert!(h.contains("ASK_MULTI"));
        assert!(h.contains("ASK_YESNO"));
        assert!(h.contains("interview-response"));
    }

    // ── state hydration + navigation ─────────────────────────────────────

    fn fixture() -> Interview {
        let src = r#"[[INTERVIEW: {
            "title": "Project setup",
            "description": "Scaffold a new app.",
            "submitLabel": "Create",
            "fields": [
                { "type": "section", "title": "Basics" },
                { "type": "text", "id": "name", "label": "Project name",
                  "required": true, "placeholder": "my-app" },
                { "type": "select", "id": "framework", "label": "Framework",
                  "options": ["React","Vue","Svelte"], "default": "Vue" },

                { "type": "section", "title": "Options" },
                { "type": "toggle", "id": "typescript", "label": "Use TypeScript?",
                  "default": true },
                { "type": "multiselect", "id": "features", "label": "Features",
                  "options": ["router","store","testing","i18n"],
                  "default": ["router","testing"] },
                { "type": "number", "id": "port", "label": "Port",
                  "min": 1024, "max": 65535, "default": 5173 }
            ]
        }]]"#;
        parse_marker(src).unwrap().0
    }

    #[test]
    fn state_hydrates_defaults() {
        let s = InterviewState::from_interview(fixture());
        assert_eq!(s.title, "Project setup");
        assert_eq!(s.submit_label, "Create");
        assert_eq!(s.fields.len(), 7); // 2 sections + 5 interactive
        // First focus skips the leading Section.
        assert!(s.fields[s.focus].is_interactive());
        match &s.fields[s.focus] {
            FieldValue::Text {
                id, placeholder, ..
            } => {
                assert_eq!(id, "name");
                assert_eq!(placeholder.as_deref(), Some("my-app"));
            }
            _ => panic!("expected Text first"),
        }
        // Select hydrates selected index from default.
        let select = s
            .fields
            .iter()
            .find_map(|f| match f {
                FieldValue::Select {
                    id,
                    selected,
                    options,
                    ..
                } if id == "framework" => Some((*selected, options.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(select.1[select.0], "Vue");
        // Multiselect hydrates per-option booleans.
        let ms = s
            .fields
            .iter()
            .find_map(|f| match f {
                FieldValue::Multiselect {
                    id,
                    checked,
                    options,
                    ..
                } if id == "features" => Some((checked.clone(), options.clone())),
                _ => None,
            })
            .unwrap();
        let picked: Vec<&String> =
            ms.1.iter()
                .zip(ms.0.iter())
                .filter_map(|(o, c)| if *c { Some(o) } else { None })
                .collect();
        assert_eq!(picked, vec![&"router".to_string(), &"testing".to_string()]);
        // Number default serialises as the crisp integer "5173".
        let num = s
            .fields
            .iter()
            .find_map(|f| match f {
                FieldValue::Number { id, value, .. } if id == "port" => Some(value.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(num, "5173");
    }

    #[test]
    fn focus_cycles_through_submit_slot() {
        // Interactive field → submit slot → back to first interactive.
        let mut s = InterviewState::from_interview(fixture());
        assert!(!s.focus_on_submit());
        // Walk forward N+1 times where N = interactive field count; we
        // should hit the submit slot exactly once per cycle.
        let interactive = s.fields.iter().filter(|f| f.is_interactive()).count();
        let mut hit_submit = 0;
        for _ in 0..(interactive + 1) {
            s.focus_next();
            if s.focus_on_submit() {
                hit_submit += 1;
            }
        }
        assert_eq!(hit_submit, 1);
    }

    #[test]
    fn focus_prev_wraps_through_submit_slot() {
        let mut s = InterviewState::from_interview(fixture());
        // From the first interactive field, going back should land on
        // the submit slot.
        s.focus_prev();
        assert!(s.focus_on_submit());
    }

    #[test]
    fn focus_navigation_skips_sections() {
        let mut s = InterviewState::from_interview(fixture());
        // Capture ids in field order for interactive entries.
        let interactive_ids: Vec<String> = s
            .fields
            .iter()
            .filter_map(|f| match f {
                FieldValue::Text { id, .. }
                | FieldValue::Toggle { id, .. }
                | FieldValue::Select { id, .. }
                | FieldValue::Multiselect { id, .. }
                | FieldValue::Number { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let mut ordered: Vec<String> = Vec::new();
        for _ in 0..interactive_ids.len() {
            let id = match &s.fields[s.focus] {
                FieldValue::Text { id, .. }
                | FieldValue::Toggle { id, .. }
                | FieldValue::Select { id, .. }
                | FieldValue::Multiselect { id, .. }
                | FieldValue::Number { id, .. } => id.clone(),
                _ => panic!(),
            };
            ordered.push(id);
            s.focus_next();
        }
        assert_eq!(ordered, interactive_ids);
    }

    #[test]
    fn missing_required_blocks_submit() {
        let s = InterviewState::from_interview(fixture());
        // `name` is required with empty default → missing.
        assert_eq!(s.first_missing_required(), Some("Project name"));
    }

    #[test]
    fn as_response_serialises_every_kind() {
        let mut s = InterviewState::from_interview(fixture());
        // Fill in the required name.
        if let FieldValue::Text { value, .. } = &mut s.fields[1] {
            *value = "my-app".into();
        }
        let resp = s.as_response();
        assert!(resp.starts_with("<interview-response>"));
        assert!(resp.ends_with("</interview-response>"));
        // Pull out the JSON body and verify keys.
        let start = resp.find('{').unwrap();
        let end = resp.rfind('}').unwrap() + 1;
        let json: serde_json::Value = serde_json::from_str(&resp[start..end]).unwrap();
        assert_eq!(json["title"], "Project setup");
        let answers = &json["answers"];
        assert_eq!(answers["name"], "my-app");
        assert_eq!(answers["framework"], "Vue");
        assert_eq!(answers["typescript"], true);
        assert_eq!(
            answers["features"],
            serde_json::json!(["router", "testing"])
        );
        assert_eq!(answers["port"], 5173.0);
    }

    // ── flat ASK markers (primary grammar — plan-mode style) ────────────

    #[test]
    fn ask_text_marker_parses_with_default() {
        let src = "Fill this in: [[ASK_TEXT: name | Project name | my-app]]";
        let (iv, ranges) = parse_ask_markers(src).expect("parses");
        assert_eq!(ranges.len(), 1);
        assert_eq!(iv.title, "Questions");
        assert_eq!(iv.fields.len(), 1);
        match &iv.fields[0] {
            Field::Text {
                id,
                label,
                default,
                required,
                multiline,
                ..
            } => {
                assert_eq!(id, "name");
                assert_eq!(label, "Project name");
                assert_eq!(default.as_deref(), Some("my-app"));
                assert!(!*required);
                assert!(!*multiline);
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn ask_bang_means_required() {
        let src = "[[ASK_TEXT!: email | Email]]";
        let (iv, _) = parse_ask_markers(src).unwrap();
        match &iv.fields[0] {
            Field::Text { required, .. } => assert!(required),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn ask_area_is_multiline() {
        let src = "[[ASK_AREA: notes | Notes]]";
        let (iv, _) = parse_ask_markers(src).unwrap();
        match &iv.fields[0] {
            Field::Text { multiline, .. } => assert!(multiline),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn ask_yesno_accepts_various_bools() {
        for (input, expected) in [
            ("yes", true),
            ("YES", true),
            ("true", true),
            ("1", true),
            ("no", false),
            ("false", false),
            ("0", false),
        ] {
            let src = format!("[[ASK_YESNO: x | X | {input}]]");
            let (iv, _) = parse_ask_markers(&src).unwrap();
            match &iv.fields[0] {
                Field::Toggle { default, .. } => assert_eq!(*default, expected),
                _ => panic!(),
            }
        }
    }

    #[test]
    fn ask_pick_star_marks_default() {
        let src = "[[ASK_PICK: fw | Framework | React | Vue* | Svelte | None]]";
        let (iv, _) = parse_ask_markers(src).unwrap();
        match &iv.fields[0] {
            Field::Select {
                options, default, ..
            } => {
                assert_eq!(options, &vec!["React", "Vue", "Svelte", "None"]);
                assert_eq!(default.as_deref(), Some("Vue"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ask_multi_stars_preselect() {
        let src = "[[ASK_MULTI: features | Features | router* | store | testing* | i18n]]";
        let (iv, _) = parse_ask_markers(src).unwrap();
        match &iv.fields[0] {
            Field::Multiselect {
                options, default, ..
            } => {
                assert_eq!(options, &vec!["router", "store", "testing", "i18n"]);
                assert_eq!(default, &vec!["router".to_string(), "testing".to_string()]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ask_num_parses_range() {
        let src = "[[ASK_NUM: port | Port | 5173 | 1024-65535]]";
        let (iv, _) = parse_ask_markers(src).unwrap();
        match &iv.fields[0] {
            Field::Number {
                default, min, max, ..
            } => {
                assert_eq!(*default, Some(5173.0));
                assert_eq!(*min, Some(1024.0));
                assert_eq!(*max, Some(65535.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ask_title_desc_submit_are_meta() {
        let src = "\
            [[ASK_TITLE: Project setup]]\
            [[ASK_DESC: Tell me about the project]]\
            [[ASK_SUBMIT: Create]]\
            [[ASK_TEXT: name | Name]]\
        ";
        let (iv, _) = parse_ask_markers(src).unwrap();
        assert_eq!(iv.title, "Project setup");
        assert_eq!(iv.description.as_deref(), Some("Tell me about the project"));
        assert_eq!(iv.submit_label.as_deref(), Some("Create"));
    }

    #[test]
    fn ask_section_and_info_are_noninteractive() {
        let src = "\
            [[ASK_SECTION: Basics]]\
            [[ASK_INFO: Any note]]\
            [[ASK_TEXT: a | A]]\
        ";
        let (iv, _) = parse_ask_markers(src).unwrap();
        assert_eq!(iv.fields.len(), 3);
        assert!(matches!(&iv.fields[0], Field::Section { title, .. } if title == "Basics"));
        assert!(matches!(&iv.fields[1], Field::Info { text } if text == "Any note"));
    }

    #[test]
    fn ask_without_interactive_returns_none() {
        // Only meta + section markers — not enough to build a real form.
        let src = "[[ASK_TITLE: T]] [[ASK_SECTION: S]] [[ASK_INFO: i]]";
        assert!(parse_ask_markers(src).is_none());
    }

    #[test]
    fn full_realistic_agent_emission() {
        let src = "\
Let me set up your project for you.

[[ASK_TITLE: Project setup]]
[[ASK_DESC: Tell me how to scaffold this]]

[[ASK_SECTION: Basics]]
[[ASK_TEXT!: name | Project name | my-app]]
[[ASK_PICK: framework | Framework | React | Vue* | Svelte | None]]

[[ASK_SECTION: Options]]
[[ASK_YESNO: typescript | Use TypeScript? | yes]]
[[ASK_MULTI: features | Include features | router* | store | testing* | i18n]]
[[ASK_NUM!: port | Dev-server port | 5173 | 1024-65535]]
[[ASK_AREA: notes | Additional notes]]
[[ASK_SUBMIT: Create]]

Once you fill that out I'll scaffold it.";
        let (iv, ranges) = parse_ask_markers(src).expect("parses full form");
        assert_eq!(iv.title, "Project setup");
        assert_eq!(iv.submit_label.as_deref(), Some("Create"));
        // 2 sections + 6 interactive fields = 8 fields total.
        assert_eq!(iv.fields.len(), 8);
        // 11 ASK markers in the text → 11 strip ranges.
        assert_eq!(ranges.len(), 11);

        // detect_interview also finds it via the primary path.
        let (iv2, _) = detect_interview(src).unwrap();
        assert_eq!(iv2.title, "Project setup");
    }

    #[test]
    fn strip_ranges_removes_markers_preserves_prose() {
        let src = "\
Let me set up your project.

[[ASK_TITLE: Setup]]
[[ASK_TEXT: name | Name]]
[[ASK_SUBMIT: Go]]

I'll take it from here.";
        let (_, ranges) = parse_ask_markers(src).unwrap();
        let stripped = strip_ranges(src, ranges);
        assert!(!stripped.contains("[["));
        assert!(stripped.contains("Let me set up your project"));
        assert!(stripped.contains("I'll take it from here"));
    }

    // ── lenient detection (fenced + bare JSON fallbacks) ─────────────────

    #[test]
    fn detect_falls_back_to_json_marker_when_no_ask_markers() {
        // Still accepted — flat ASK markers are primary but the JSON
        // marker path is supported for agents that prefer it.
        let src = r#"lorem [[INTERVIEW: {"title":"M","fields":[
            {"type":"text","id":"a","label":"A"}
        ]}]] ipsum"#;
        let (iv, _) = detect_interview(src).unwrap();
        assert_eq!(iv.title, "M");
    }

    #[test]
    fn detect_prefers_ask_markers_over_json_fallback() {
        // When both ASK markers and JSON are present, the flat marker
        // path wins.
        let src = "\
[[ASK_TITLE: Flat]]
[[ASK_TEXT: a | A]]
Also here's JSON: [[INTERVIEW: {\"title\":\"JSON\",\"fields\":[{\"type\":\"text\",\"id\":\"b\",\"label\":\"B\"}]}]]
";
        let (iv, _) = detect_interview(src).unwrap();
        assert_eq!(iv.title, "Flat");
    }

    #[test]
    fn detect_fenced_code_block() {
        let src = r#"Here's the form:

```json
{
  "title": "Setup",
  "fields": [
    { "type": "text", "id": "name", "label": "Name" }
  ]
}
```

Please fill it out."#;
        let (iv, ranges) = detect_interview(src).expect("detects fenced");
        assert_eq!(iv.title, "Setup");
        // The range must include the entire fenced block so stripping
        // removes the triple-backticks too.
        let r = &ranges[0];
        assert!(src[r.start..r.end].starts_with("```"));
        assert!(src[r.start..r.end].ends_with("```"));
    }

    #[test]
    fn detect_fenced_block_any_language_tag() {
        // `json-interview` or missing tag — both work.
        let src = "blah\n```interview\n{\"title\":\"T\",\"fields\":[{\"type\":\"toggle\",\"id\":\"x\",\"label\":\"X\"}]}\n```\n";
        let (iv, _) = detect_interview(src).expect("detects");
        assert_eq!(iv.title, "T");
    }

    #[test]
    fn detect_bare_json() {
        let src = r#"Here's the form you need:

{
  "title": "Onboarding",
  "fields": [
    { "type": "text", "id": "email", "label": "Email", "required": true }
  ]
}

End of message."#;
        let (iv, ranges) = detect_interview(src).expect("detects bare json");
        assert_eq!(iv.title, "Onboarding");
        let r = &ranges[0];
        assert!(src[r.start..r.end].trim().starts_with('{'));
        assert!(src[r.start..r.end].trim().ends_with('}'));
    }

    #[test]
    fn detect_rejects_accidental_json() {
        // Valid JSON object but not interview-shaped.
        let src = r#"Config:
```json
{"foo": "bar", "baz": 1}
```"#;
        assert!(detect_interview(src).is_none());
    }

    #[test]
    fn detect_rejects_empty_fields_list() {
        let src = r#"```json
{"title":"Empty","fields":[]}
```"#;
        assert!(detect_interview(src).is_none());
    }

    #[test]
    fn detect_rejects_only_sections() {
        // Has fields but none interactive — doesn't count as an interview.
        let src = r#"```json
{"title":"T","fields":[{"type":"section","title":"S"}]}
```"#;
        assert!(detect_interview(src).is_none());
    }

    #[test]
    fn strip_range_collapses_newlines() {
        let text = "before\n\n\n\nafter";
        let out = strip_range(text, 6..8);
        // The removed range leaves 2+ newlines which we collapse to 2 max,
        // and surrounding whitespace is trimmed.
        assert!(out.starts_with("before"));
        assert!(out.ends_with("after"));
        assert!(!out.contains("\n\n\n\n"));
    }

    #[test]
    fn human_summary_formats_answers() {
        let mut s = InterviewState::from_interview(fixture());
        if let FieldValue::Text { value, .. } = &mut s.fields[1] {
            *value = "x".into();
        }
        let sum = s.human_summary();
        assert!(sum.contains("name=x"));
        assert!(sum.contains("typescript=yes"));
        assert!(sum.contains("features=[router,testing]"));
    }
}
