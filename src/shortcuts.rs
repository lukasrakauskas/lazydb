//! Keyboard shortcuts: a colocated, view-aware keymap.
//!
//! SOLID layout:
//! - `View` / `Action` / `Binding` are the data: which screen, which
//!   behavior, which keys map to it, plus a label for the help bar.
//! - `current_view` / `match_key` / `active` resolve + dispatch with NO
//!   dependency on `App` (dependency inversion: `App` calls these).
//! - The handler (`App::apply_action` in `app.rs`) is the single place that
//!   maps `Action` → mutation. Adding a shortcut = add a `Binding` here + an
//!   arm there; the dispatcher never changes (open/closed).
//! - Raw text input (typing into the editor / a form field) is NOT a
//!   shortcut — it's a fall-through, has no label, never hits the help bar.
//!
//! The handler (`App::apply_action`) lives in `app.rs` and is the single place
//! that maps `Action` → mutation. Raw text input (typing into the editor/form
//! field) is NOT a shortcut and stays a fall-through, not an action — it has no
//! label and never belongs in the help bar.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Focus;

/// The active context, resolved from modal flags + focus. Modals win over focus
/// because they grab all input; `EditorAutocomplete` is a transient sub-mode of
/// `Editor` (the completion popup) that overrides a few keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    ConfirmDestructive,
    ConfirmDelete,
    Form,
    Features,
    Connections,
    Editor,
    EditorAutocomplete,
    Results,
    ResultsFilter,
    ResultsEdit,
    Schema,
}

/// Behavior a shortcut can trigger. One variant per distinct effect; the same
/// `Action` may be bound in several views (e.g. `MoveDown` in
/// Connections/Results/Schema) and is interpreted per-view by the handler.
/// Flatten instead of carrying data (`FocusEditor` over `FocusPane(Focus)`) so
/// the match is exhaustive and each binding's label is trivial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    // global (every view)
    Quit,
    RunQuery,
    // non-editor pane chrome (Connections/Results/Schema)
    FocusNext,
    FocusConnections,
    FocusEditor,
    FocusResults,
    FocusSchema,
    ToggleKeyLog,
    ToggleFeatures,
    // shared list nav — behavior is selected per-view by the handler
    MoveDown,
    MoveUp,
    MoveRight,
    MoveLeft,
    PageDown,
    PageUp,
    Home,
    End,
    // connections
    ConnectSelected,
    NewConnection,
    DeleteConnection,
    EditConnection,
    // editor
    EditorNewline,
    EditorBackspace,
    EditorLeft,
    EditorRight,
    EditorUp,
    EditorDown,
    EditorHome,
    EditorEnd,
    RecallHistoryOlder,
    RecallHistoryNewer,
    // autocomplete popup (editor sub-mode)
    AcceptCompletion,
    CompletionNext,
    CompletionPrev,
    DismissCompletion,
    // results
    CopyRowJson,
    CopyResultCsv,
    Deselect,
    // results fuzzy filter (a transient input mode of the Results view)
    ToggleFilter,
    FilterAccept,
    FilterCancel,
    /// Filter input row: delete the char before the cursor (Backspace).
    FilterBackspace,
    // results inline cell editing
    EditCell,
    EditCellConfirm,
    EditCellCancel,
    EditCellLeft,
    EditCellRight,
    EditCellBackspace,
    EditCellHome,
    EditCellEnd,
    // schema
    SchemaExpand,
    SchemaCollapse,
    // form modal
    FormSave,
    FormCancel,
    FormTestConnection,
    FormFieldNext,
    FormFieldPrev,
    FormFieldLeft,
    FormFieldRight,
    FormFieldHome,
    FormFieldEnd,
    FormFieldBackspace,
    FormCycleKind,
    // features modal
    FeaturesClose,
    FeaturesNext,
    FeaturesPrev,
    FeaturesToggle,
    // confirm-destructive modal
    ConfirmYes,
    ConfirmNo,
}

/// A key chord to match: `code` plus modifiers that must be present (`require`)
/// and must be absent (`forbid`). Stored as raw bits so binding tables can be
/// `const` — crossterm's `KeyModifiers |` is not a const op, but `u8 |` is.
/// `display` is the help-bar string for this one chord.
#[derive(Clone, Copy, Debug)]
pub struct KeyPattern {
    code: KeyCode,
    require: u8,
    forbid: u8,
    display: &'static str,
}

const CTRL: u8 = KeyModifiers::CONTROL.bits();
const ALT: u8 = KeyModifiers::ALT.bits();
const SHIFT: u8 = KeyModifiers::SHIFT.bits();

/// Plain char, no Ctrl/Alt. Shift is fine — it changes the char's case, so the
/// `code` already encodes it (e.g. Shift+'q' arrives as `Char('Q')`, not this).
const fn ch(c: char, d: &'static str) -> KeyPattern {
    KeyPattern {
        code: KeyCode::Char(c),
        require: 0,
        forbid: CTRL | ALT,
        display: d,
    }
}
/// Ctrl+char.
const fn ctrl(c: char, d: &'static str) -> KeyPattern {
    KeyPattern {
        code: KeyCode::Char(c),
        require: CTRL,
        forbid: 0,
        display: d,
    }
}
/// Shift+<special key> (arrows), no Ctrl/Alt.
const fn shift(code: KeyCode, d: &'static str) -> KeyPattern {
    KeyPattern {
        code,
        require: SHIFT,
        forbid: CTRL | ALT,
        display: d,
    }
}
/// Alt+<special key>.
const fn alt(code: KeyCode, d: &'static str) -> KeyPattern {
    KeyPattern {
        code,
        require: ALT,
        forbid: 0,
        display: d,
    }
}
/// Bare special key (arrows/PgUp/Esc/F-keys). Shift+Tab is a distinct
/// `BackTab` KeyCode, so `bare(Tab)` still won't match it.
const fn bare(code: KeyCode, d: &'static str) -> KeyPattern {
    KeyPattern {
        code,
        require: 0,
        forbid: 0,
        display: d,
    }
}

impl KeyPattern {
    pub fn matches(&self, key: &KeyEvent) -> bool {
        let req = KeyModifiers::from_bits_truncate(self.require);
        let forb = KeyModifiers::from_bits_truncate(self.forbid);
        key.code == self.code && key.modifiers.contains(req) && !key.modifiers.intersects(forb)
    }
}

/// One shortcut: any of `keys` triggers `action`; `label` is the help-bar verb.
/// Within a view's table, list more-specific bindings (modifier-required)
/// before less-specific ones so e.g. Shift+Up wins over plain Up.
pub struct Binding {
    pub keys: &'static [KeyPattern],
    pub label: &'static str,
    pub action: Action,
    pub hidden: bool,
}

impl Binding {
    /// Joined key display for the help bar, e.g. "Ctrl+R/F5/Opt+Enter".
    /// ponytail: small per-frame alloc (a few short joins at 10fps); cache the
    /// string on `Binding` if profiling ever shows it.
    pub fn keys_display(&self) -> String {
        let mut s = String::new();
        for (i, k) in self.keys.iter().enumerate() {
            if i > 0 {
                s.push('/');
            }
            s.push_str(k.display);
        }
        s
    }
}

// --- binding tables -------------------------------------------------------

/// Active in every view, checked last (so a view can override). Quit + run are
/// genuinely global (they work inside modals too — matches prior behavior).
pub static GLOBAL: &[Binding] = &[
    Binding {
        keys: &[ctrl('c', "Ctrl+C"), ctrl('q', "Ctrl+Q")],
        label: "quit",
        action: Action::Quit,
        hidden: false,
    },
    Binding {
        keys: &[
            ctrl('r', "Ctrl+R"),
            bare(KeyCode::F(5), "F5"),
            alt(KeyCode::Enter, "Opt+Enter"),
        ],
        label: "run",
        action: Action::RunQuery,
        hidden: false,
    },
];

/// Pane chrome active in the non-editor, non-modal panes. `focus != Editor` in
/// the old code → exactly these three views.
pub static COMMON_PANE: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Tab, "Tab")],
        label: "focus",
        action: Action::FocusNext,
        hidden: false,
    },
    Binding {
        keys: &[ch('1', "1")],
        label: "conn",
        action: Action::FocusConnections,
        hidden: true,
    },
    Binding {
        keys: &[ch('2', "2")],
        label: "editor",
        action: Action::FocusEditor,
        hidden: true,
    },
    Binding {
        keys: &[ch('3', "3")],
        label: "results",
        action: Action::FocusResults,
        hidden: true,
    },
    Binding {
        keys: &[ch('4', "4")],
        label: "schema",
        action: Action::FocusSchema,
        hidden: true,
    },
    Binding {
        keys: &[ch('?', "?")],
        label: "key-log",
        action: Action::ToggleKeyLog,
        hidden: false,
    },
    Binding {
        keys: &[ch('f', "f")],
        label: "features",
        action: Action::ToggleFeatures,
        hidden: false,
    },
    Binding {
        keys: &[ch('q', "q")],
        label: "quit",
        action: Action::Quit,
        hidden: false,
    },
];

static CONNECTIONS: &[Binding] = &[
    Binding {
        keys: &[ch('j', "j"), bare(KeyCode::Down, "↓")],
        label: "down",
        action: Action::MoveDown,
        hidden: false,
    },
    Binding {
        keys: &[ch('k', "k"), bare(KeyCode::Up, "↑")],
        label: "up",
        action: Action::MoveUp,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "connect",
        action: Action::ConnectSelected,
        hidden: false,
    },
    Binding {
        keys: &[ch('n', "n")],
        label: "new",
        action: Action::NewConnection,
        hidden: false,
    },
    Binding {
        keys: &[ch('d', "d")],
        label: "delete",
        action: Action::DeleteConnection,
        hidden: false,
    },
    Binding {
        keys: &[ch('e', "e")],
        label: "edit",
        action: Action::EditConnection,
        hidden: false,
    },
];

static EDITOR: &[Binding] = &[
    // specific-first: Shift+Up/Down (history recall) before plain Up/Down.
    Binding {
        keys: &[shift(KeyCode::Up, "Shift+↑")],
        label: "hist-prev",
        action: Action::RecallHistoryOlder,
        hidden: false,
    },
    Binding {
        keys: &[shift(KeyCode::Down, "Shift+↓")],
        label: "hist-next",
        action: Action::RecallHistoryNewer,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Up, "↑")],
        label: "up",
        action: Action::EditorUp,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Down, "↓")],
        label: "down",
        action: Action::EditorDown,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Left, "←")],
        label: "left",
        action: Action::EditorLeft,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Right, "→")],
        label: "right",
        action: Action::EditorRight,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Home, "Home")],
        label: "home",
        action: Action::EditorHome,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::End, "End")],
        label: "end",
        action: Action::EditorEnd,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "newline",
        action: Action::EditorNewline,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Backspace, "⌫")],
        label: "del",
        action: Action::EditorBackspace,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Tab, "Tab")],
        label: "focus",
        action: Action::FocusNext,
        hidden: false,
    },
];

static EDITOR_AUTOCOMPLETE: &[Binding] = &[
    Binding {
        keys: &[shift(KeyCode::Up, "Shift+↑")],
        label: "hist-prev",
        action: Action::RecallHistoryOlder,
        hidden: false,
    },
    Binding {
        keys: &[shift(KeyCode::Down, "Shift+↓")],
        label: "hist-next",
        action: Action::RecallHistoryNewer,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Tab, "Tab"), bare(KeyCode::Enter, "⏎")],
        label: "accept",
        action: Action::AcceptCompletion,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Down, "↓")],
        label: "next",
        action: Action::CompletionNext,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Up, "↑")],
        label: "prev",
        action: Action::CompletionPrev,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc")],
        label: "dismiss",
        action: Action::DismissCompletion,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Left, "←")],
        label: "left",
        action: Action::EditorLeft,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Right, "→")],
        label: "right",
        action: Action::EditorRight,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Home, "Home")],
        label: "home",
        action: Action::EditorHome,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::End, "End")],
        label: "end",
        action: Action::EditorEnd,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Backspace, "⌫")],
        label: "del",
        action: Action::EditorBackspace,
        hidden: false,
    },
];

static RESULTS: &[Binding] = &[
    Binding {
        keys: &[ch('j', "j"), bare(KeyCode::Down, "↓")],
        label: "row↓",
        action: Action::MoveDown,
        hidden: false,
    },
    Binding {
        keys: &[ch('k', "k"), bare(KeyCode::Up, "↑")],
        label: "row↑",
        action: Action::MoveUp,
        hidden: false,
    },
    Binding {
        keys: &[ch('l', "l"), bare(KeyCode::Right, "→")],
        label: "col→",
        action: Action::MoveRight,
        hidden: false,
    },
    Binding {
        keys: &[ch('h', "h"), bare(KeyCode::Left, "←")],
        label: "col←",
        action: Action::MoveLeft,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::PageDown, "PgDn")],
        label: "pg↓",
        action: Action::PageDown,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::PageUp, "PgUp")],
        label: "pg↑",
        action: Action::PageUp,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Home, "Home")],
        label: "top",
        action: Action::Home,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::End, "End")],
        label: "bottom",
        action: Action::End,
        hidden: false,
    },
    Binding {
        keys: &[ch('y', "y")],
        label: "copy row",
        action: Action::CopyRowJson,
        hidden: false,
    },
    Binding {
        keys: &[ch('/', "/")],
        label: "filter",
        action: Action::ToggleFilter,
        hidden: false,
    },
    Binding {
        keys: &[ch('e', "e")],
        label: "edit cell",
        action: Action::EditCell,
        hidden: false,
    },
    Binding {
        keys: &[ch('d', "d")],
        label: "deselect",
        action: Action::Deselect,
        hidden: false,
    },
    Binding {
        keys: &[ctrl('s', "Ctrl+S")],
        label: "copy CSV",
        action: Action::CopyResultCsv,
        hidden: false,
    },
];

static RESULTS_FILTER: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "accept",
        action: Action::FilterAccept,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc")],
        label: "cancel",
        action: Action::FilterCancel,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Backspace, "⌫")],
        label: "del",
        action: Action::FilterBackspace,
        hidden: false,
    },
    Binding {
        keys: &[ch('/', "/")],
        label: "close",
        action: Action::ToggleFilter,
        hidden: false,
    },
    // typed chars fall through to raw text input (the filter query); no binding.
];

static RESULTS_EDIT: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "save",
        action: Action::EditCellConfirm,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc")],
        label: "cancel",
        action: Action::EditCellCancel,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Left, "←")],
        label: "left",
        action: Action::EditCellLeft,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Right, "→")],
        label: "right",
        action: Action::EditCellRight,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Home, "Home")],
        label: "home",
        action: Action::EditCellHome,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::End, "End")],
        label: "end",
        action: Action::EditCellEnd,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Backspace, "⌫")],
        label: "del",
        action: Action::EditCellBackspace,
        hidden: false,
    },
    // typed chars fall through to raw text input; no binding.
];

static SCHEMA: &[Binding] = &[
    Binding {
        keys: &[ch('j', "j"), bare(KeyCode::Down, "↓")],
        label: "down",
        action: Action::MoveDown,
        hidden: false,
    },
    Binding {
        keys: &[ch('k', "k"), bare(KeyCode::Up, "↑")],
        label: "up",
        action: Action::MoveUp,
        hidden: false,
    },
    Binding {
        keys: &[
            bare(KeyCode::Enter, "⏎"),
            ch('l', "l"),
            bare(KeyCode::Right, "→"),
        ],
        label: "expand",
        action: Action::SchemaExpand,
        hidden: false,
    },
    Binding {
        keys: &[ch('h', "h"), bare(KeyCode::Left, "←")],
        label: "collapse",
        action: Action::SchemaCollapse,
        hidden: false,
    },
];

static FORM: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc")],
        label: "cancel",
        action: Action::FormCancel,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "save",
        action: Action::FormSave,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Tab, "Tab"), bare(KeyCode::Down, "↓")],
        label: "next",
        action: Action::FormFieldNext,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::BackTab, "Shift+Tab"), bare(KeyCode::Up, "↑")],
        label: "prev",
        action: Action::FormFieldPrev,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Left, "←")],
        label: "left",
        action: Action::FormFieldLeft,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Right, "→")],
        label: "right",
        action: Action::FormFieldRight,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Home, "Home")],
        label: "home",
        action: Action::FormFieldHome,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::End, "End")],
        label: "end",
        action: Action::FormFieldEnd,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Backspace, "⌫")],
        label: "del",
        action: Action::FormFieldBackspace,
        hidden: false,
    },
    Binding {
        keys: &[ctrl('t', "Ctrl+T")],
        label: "test",
        action: Action::FormTestConnection,
        hidden: false,
    },
    Binding {
        // ponytail: Ctrl+K cycles the Type row (Kind). One key, wraps; with
        // 2 backends it's a toggle. Add to KINDS to extend.
        keys: &[ctrl('k', "Ctrl+K")],
        label: "type",
        action: Action::FormCycleKind,
        hidden: false,
    },
];

static FEATURES: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc"), ch('f', "f"), ch('q', "q")],
        label: "close",
        action: Action::FeaturesClose,
        hidden: false,
    },
    Binding {
        keys: &[ch('j', "j"), bare(KeyCode::Down, "↓")],
        label: "down",
        action: Action::FeaturesNext,
        hidden: false,
    },
    Binding {
        keys: &[ch('k', "k"), bare(KeyCode::Up, "↑")],
        label: "up",
        action: Action::FeaturesPrev,
        hidden: false,
    },
    Binding {
        keys: &[ch(' ', "Space"), bare(KeyCode::Enter, "⏎")],
        label: "toggle",
        action: Action::FeaturesToggle,
        hidden: false,
    },
];

static CONFIRM: &[Binding] = &[
    Binding {
        keys: &[ch('y', "y"), ch('Y', "Y")],
        label: "confirm",
        action: Action::ConfirmYes,
        hidden: false,
    },
    Binding {
        keys: &[ch('n', "n"), ch('N', "N"), bare(KeyCode::Esc, "Esc")],
        label: "cancel",
        action: Action::ConfirmNo,
        hidden: false,
    },
];

static DELETE_CONFIRM: &[Binding] = &[
    Binding {
        keys: &[bare(KeyCode::Enter, "⏎")],
        label: "confirm",
        action: Action::ConfirmYes,
        hidden: false,
    },
    Binding {
        keys: &[bare(KeyCode::Esc, "Esc")],
        label: "cancel",
        action: Action::ConfirmNo,
        hidden: false,
    },
];

fn view_bindings(view: View) -> &'static [Binding] {
    match view {
        View::Connections => CONNECTIONS,
        View::Editor => EDITOR,
        View::EditorAutocomplete => EDITOR_AUTOCOMPLETE,
        View::Results => RESULTS,
        View::ResultsFilter => RESULTS_FILTER,
        View::ResultsEdit => RESULTS_EDIT,
        View::Schema => SCHEMA,
        View::Form => FORM,
        View::Features => FEATURES,
        View::ConfirmDestructive => CONFIRM,
        View::ConfirmDelete => DELETE_CONFIRM,
    }
}

fn common_for(view: View) -> &'static [Binding] {
    match view {
        View::Connections | View::Results | View::Schema => COMMON_PANE,
        _ => &[],
    }
}

/// All bindings active in `view`, in precedence order: view-specific (may
// override), then common pane chrome, then global. Used by `match_key`
// (first match wins) and by the help bar (`bar_bindings`).
pub fn active(view: View) -> impl Iterator<Item = &'static Binding> {
    view_bindings(view)
        .iter()
        .chain(common_for(view).iter())
        .chain(GLOBAL.iter())
}

/// The subset of `active(view)` shown in the bottom shortcuts bar. The
// `1`/`2`/`3`/`4` pane-focus bindings are excluded because their affordance
// is already visible as the `[1]`..`[4]` badges on each pane's border —
// repeating them in the bar would be noise. They still match (see `active`);
// the exclusion is data-driven via `hidden: true` on the binding.
pub fn bar_bindings(view: View) -> impl Iterator<Item = &'static Binding> {
    active(view).filter(|b| !b.hidden)
}

/// Resolve the active view from app state. Modals win over focus; autocomplete
/// is a sub-mode of the editor. Takes primitives, not `&App`, so this module
/// stays decoupled from the app.
// ponytail: 8 boolean flags instead of a struct — each maps 1:1 to a UI state,
// collapsing them adds ceremony without clarity. ceiling: if a 9th lands, pack
// them into a `ViewInputs` struct.
#[allow(clippy::too_many_arguments)]
pub fn current_view(
    focus: Focus,
    form: bool,
    features: bool,
    confirm_destructive: bool,
    confirm_delete: bool,
    autocomplete: bool,
    filter_input_open: bool,
    edit_cell: bool,
) -> View {
    if confirm_destructive {
        View::ConfirmDestructive
    } else if confirm_delete {
        View::ConfirmDelete
    } else if form {
        View::Form
    } else if features {
        View::Features
    } else if filter_input_open {
        View::ResultsFilter
    } else if edit_cell {
        View::ResultsEdit
    } else if autocomplete {
        View::EditorAutocomplete
    } else {
        match focus {
            Focus::Connections => View::Connections,
            Focus::Editor => View::Editor,
            Focus::Results => View::Results,
            Focus::Schema => View::Schema,
        }
    }
}

/// Find the action for `key` in the active view, or `None` when no shortcut
/// matches (caller falls through to raw text input).
pub fn match_key(view: View, key: KeyEvent) -> Option<Action> {
    active(view)
        .find(|b| b.keys.iter().any(|p| p.matches(&key)))
        .map(|b| b.action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn k(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind_and_state(code, mods, KeyEventKind::Press, KeyEventState::NONE)
    }

    #[test]
    fn current_view_modal_wins_over_focus_and_autocomplete() {
        assert_eq!(
            current_view(Focus::Editor, true, true, true, false, true, true, false),
            View::ConfirmDestructive
        );
        assert_eq!(
            current_view(
                Focus::Editor,
                true,
                false,
                false,
                false,
                false,
                false,
                false
            ),
            View::Form
        );
        assert_eq!(
            current_view(
                Focus::Editor,
                false,
                true,
                false,
                false,
                false,
                false,
                false
            ),
            View::Features
        );
        assert_eq!(
            current_view(
                Focus::Editor,
                false,
                false,
                false,
                false,
                true,
                false,
                false
            ),
            View::EditorAutocomplete
        );
        assert_eq!(
            current_view(
                Focus::Results,
                false,
                false,
                false,
                false,
                false,
                false,
                false
            ),
            View::Results
        );
        // input open → ResultsFilter (typing mode). The "filter applied but
        // input closed" case (after Accept) is just the line above — Results —
        // since current_view only takes the open flag, not whether a filter is applied.
        assert_eq!(
            current_view(
                Focus::Results,
                false,
                false,
                false,
                false,
                false,
                true,
                false
            ),
            View::ResultsFilter
        );
        // edit_cell → ResultsEdit
        assert_eq!(
            current_view(
                Focus::Results,
                false,
                false,
                false,
                false,
                false,
                false,
                true
            ),
            View::ResultsEdit
        );
        // edit_cell loses to modals
        assert_eq!(
            current_view(
                Focus::Results,
                false,
                false,
                true,
                false,
                false,
                false,
                true
            ),
            View::ConfirmDestructive
        );
        // confirm_delete view
        assert_eq!(
            current_view(
                Focus::Results,
                false,
                false,
                false,
                true,
                false,
                false,
                false
            ),
            View::ConfirmDelete
        );
    }

    #[test]
    fn results_view_binds_nav_copy_and_chrome() {
        let v = View::Results;
        assert_eq!(
            match_key(v, k(KeyCode::Char('y'), KeyModifiers::NONE)),
            Some(Action::CopyRowJson)
        );
        // Ctrl+y must NOT copy a row (plain-char bindings forbid Ctrl).
        assert_eq!(
            match_key(v, k(KeyCode::Char('y'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            Some(Action::CopyResultCsv)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('j'), KeyModifiers::NONE)),
            Some(Action::MoveDown)
        );
        // common pane chrome
        assert_eq!(
            match_key(v, k(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(Action::FocusConnections)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Quit)
        );
        // global
        assert_eq!(
            match_key(v, k(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            match_key(v, k(KeyCode::F(5), KeyModifiers::NONE)),
            Some(Action::RunQuery)
        );
        // Tab → focus next (common), not a result nav key
        assert_eq!(
            match_key(v, k(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::FocusNext)
        );
        // unbound plain char falls through
        assert_eq!(
            match_key(v, k(KeyCode::Char('z'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn editor_shift_arrows_win_over_plain_arrows() {
        let v = View::Editor;
        assert_eq!(
            match_key(v, k(KeyCode::Up, KeyModifiers::SHIFT)),
            Some(Action::RecallHistoryOlder)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Down, KeyModifiers::SHIFT)),
            Some(Action::RecallHistoryNewer)
        );
        // plain arrows are editor cursor moves, not history recall
        assert_eq!(
            match_key(v, k(KeyCode::Up, KeyModifiers::NONE)),
            Some(Action::EditorUp)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::EditorDown)
        );
        // a typed letter is NOT a shortcut (raw text input fall-through)
        assert_eq!(
            match_key(v, k(KeyCode::Char('a'), KeyModifiers::NONE)),
            None
        );
        // global run still reaches the editor
        assert_eq!(
            match_key(v, k(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            Some(Action::RunQuery)
        );
        // Tab cycles focus from the editor (editor-specific, not common)
        assert_eq!(
            match_key(v, k(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::FocusNext)
        );
        // digits are NOT bound in the editor (they type)
        assert_eq!(
            match_key(v, k(KeyCode::Char('2'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn autocomplete_overrides_tab_and_arrows() {
        let v = View::EditorAutocomplete;
        assert_eq!(
            match_key(v, k(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::AcceptCompletion)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::AcceptCompletion)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::CompletionNext)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::DismissCompletion)
        );
        // Shift+Up still recalls history with the popup open
        assert_eq!(
            match_key(v, k(KeyCode::Up, KeyModifiers::SHIFT)),
            Some(Action::RecallHistoryOlder)
        );
        // typed char falls through to editor insert (not a shortcut)
        assert_eq!(
            match_key(v, k(KeyCode::Char('a'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn features_modal_q_closes_and_ctrl_q_quits() {
        let v = View::Features;
        assert_eq!(
            match_key(v, k(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::FeaturesClose)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(Action::FeaturesClose)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char(' '), KeyModifiers::NONE)),
            Some(Action::FeaturesToggle)
        );
        // Ctrl+Q is global quit, not shadowed by the modal's plain-q binding
        assert_eq!(
            match_key(v, k(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        // plain-q in the modal does NOT quit (it closes) — distinct from a list pane
        assert_ne!(
            match_key(v, k(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn schema_enter_l_right_expand_and_h_left_collapse() {
        let v = View::Schema;
        assert_eq!(
            match_key(v, k(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::SchemaExpand)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('l'), KeyModifiers::NONE)),
            Some(Action::SchemaExpand)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::SchemaExpand)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('h'), KeyModifiers::NONE)),
            Some(Action::SchemaCollapse)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::SchemaCollapse)
        );
    }

    #[test]
    fn form_tab_and_down_both_advance_field() {
        let v = View::Form;
        assert_eq!(
            match_key(v, k(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::FormFieldNext)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::FormFieldNext)
        );
        assert_eq!(
            match_key(v, k(KeyCode::BackTab, KeyModifiers::NONE)),
            Some(Action::FormFieldPrev)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::FormSave)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::FormCancel)
        );
        // typed char falls through to the field
        assert_eq!(
            match_key(v, k(KeyCode::Char('a'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn confirm_modal_y_n() {
        let v = View::ConfirmDestructive;
        assert_eq!(
            match_key(v, k(KeyCode::Char('y'), KeyModifiers::NONE)),
            Some(Action::ConfirmYes)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
            Some(Action::ConfirmYes)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Char('n'), KeyModifiers::NONE)),
            Some(Action::ConfirmNo)
        );
        assert_eq!(
            match_key(v, k(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::ConfirmNo)
        );
    }

    #[test]
    fn active_results_includes_global_run_and_view_copy() {
        // The help bar must surface both the view's own bindings and global ones.
        let actions: Vec<Action> = active(View::Results).map(|b| b.action).collect();
        assert!(
            actions.contains(&Action::CopyRowJson),
            "missing copy-row in results bar"
        );
        assert!(
            actions.contains(&Action::RunQuery),
            "missing global run in results bar"
        );
        assert!(
            actions.contains(&Action::Quit),
            "missing global quit in results bar"
        );
        assert!(
            actions.contains(&Action::FocusConnections),
            "missing common pane chrome"
        );
    }

    #[test]
    fn bar_hides_pane_focus_but_keys_still_match() {
        // 1/2/3/4 are shown on the pane badges, not the bar — but they must
        // still fire when pressed.
        let bar: Vec<Action> = bar_bindings(View::Results).map(|b| b.action).collect();
        assert!(
            !bar.contains(&Action::FocusConnections),
            "1 should be hidden from bar"
        );
        assert!(
            !bar.contains(&Action::FocusEditor),
            "2 should be hidden from bar"
        );
        assert!(
            !bar.contains(&Action::FocusResults),
            "3 should be hidden from bar"
        );
        assert!(
            !bar.contains(&Action::FocusSchema),
            "4 should be hidden from bar"
        );
        // ...yet the keymap still routes them (common pane chrome is active
        // in the non-editor panes):
        assert_eq!(
            match_key(View::Results, k(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::FocusEditor)
        );
        // editor view: digits type, so 2 is NOT bound there.
        assert_eq!(
            match_key(View::Editor, k(KeyCode::Char('2'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn keys_display_joins_with_slash() {
        // The run binding has 3 keys; the help bar shows them as one group.
        let run = GLOBAL
            .iter()
            .find(|b| b.action == Action::RunQuery)
            .unwrap();
        assert_eq!(run.keys_display(), "Ctrl+R/F5/Opt+Enter");
        assert_eq!(run.label, "run");
    }
}
