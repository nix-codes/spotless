//! What the interface asks for. Views only emit these; `App` applies them
//! after the frame is drawn: the core's through the controller, the
//! desktop's own itself.

pub use spotless_core::action::Action;

/// What only the desktop does, beside the core's [`Action`]s.
#[derive(Clone, Debug, PartialEq)]
pub enum Desktop {
    /// Quits, even with the tray keeping the window.
    Quit,
    /// The tray icon on or off.
    SetTray(bool),
    /// Draws with this built-in skin (None: Spotless Classic).
    SetSkin(Option<String>),
    /// Colours the lists and the display like this monitor.
    SetPhosphor(crate::ui::theme::Phosphor),
    /// Looking for updates once a day on or off.
    SetUpdates(bool),
    /// Looks for an update now.
    CheckForUpdate,
    /// Installs the update found.
    Update,
    /// Quits and starts the update installed.
    Restart,
    /// Fetches a cover the catalog wants to show, unless it has it.
    LoadCover(String),
}

/// An action of either kind.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyAction {
    Core(Action),
    Desktop(Desktop),
}

impl From<Action> for AnyAction {
    fn from(action: Action) -> Self {
        Self::Core(action)
    }
}

impl From<Desktop> for AnyAction {
    fn from(action: Desktop) -> Self {
        Self::Desktop(action)
    }
}

/// The actions the views emitted, in order.
#[derive(Debug, Default)]
pub struct Actions(Vec<AnyAction>);

impl Actions {
    pub fn push(&mut self, action: impl Into<AnyAction>) {
        self.0.push(action.into());
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The core's actions among them.
    pub fn core(&self) -> impl Iterator<Item = &Action> {
        self.0.iter().filter_map(|action| match action {
            AnyAction::Core(action) => Some(action),
            AnyAction::Desktop(_) => None,
        })
    }
}

impl IntoIterator for Actions {
    type Item = AnyAction;
    type IntoIter = std::vec::IntoIter<AnyAction>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
