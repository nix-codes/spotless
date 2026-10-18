//! What the interface asks for. Views only emit these; `App` applies them
//! after the frame is drawn.

use crate::model::EntryId;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SignIn,
    SignOut,
    /// Appends a track, album or artist link's songs to the viewed playlist.
    AddLink(String),
    Play,
    TogglePause,
    Stop,
    Next,
    Previous,
    Seek(u32),
    /// 0–100, as the slider shows it.
    Volume(u8),
    SetShuffle(bool),
    SetRepeat(bool),
    /// Double-click or Enter on a row of the viewed playlist.
    PlayEntry(EntryId),
    Remove(Vec<EntryId>),
    /// Moves these rows, keeping their order, to just before the row now at
    /// this index.
    Move(Vec<EntryId>, usize),
    ToggleMarks(Vec<EntryId>),
}
