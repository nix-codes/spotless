//! What the interface asks for. Views only emit these; `App` applies them
//! after the frame is drawn.

use crate::model::catalog::ReleaseTab;
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SignIn,
    SignOut,
    /// Appends a track, album or artist link's songs to the target playlist.
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

    // Playlists (tabs)
    SelectPlaylist(PlaylistId),
    NewPlaylist,
    /// Deletes a playlist and its songs (never Default).
    ClosePlaylist(PlaylistId),
    RenamePlaylist(PlaylistId, String),
    /// Moves a tab to this index, counted after it is taken out.
    MovePlaylist(PlaylistId, usize),

    // The catalog
    ToggleBrowse,
    CloseBrowse,
    Back,
    Search(String),
    OpenArtist(ArtistRef),
    /// Shows or hides an album's songs, reading them if needed.
    ToggleAlbum(String),
    /// Shows these albums' songs, or hides them all with an empty list.
    ExpandAlbums(Vec<String>),
    SetTab(ReleaseTab),
    ToggleSort,
    SetTarget(PlaylistId),
    /// Appends songs to the target playlist.
    AddTracks(Vec<TrackRef>),
    /// Appends an album's songs to the target playlist.
    AddAlbum(String),
    /// Songs dropped on the slim column, before the row at this index.
    DropTracks(Vec<TrackRef>, usize),
    /// An album dropped on the slim column, before the row at this index.
    DropAlbum(String, usize),
    /// Puts a song after the target playlist's current one and plays it.
    PlayNow(TrackRef),
}
