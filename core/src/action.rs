//! What an interface asks for. It only emits these; the controller applies
//! them. A desktop keeps actions of its own beside them (Spotless's: quitting,
//! the tray, skins, covers).

use crate::backend::Store;
use crate::model::catalog::ReleaseTab;
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SignIn,
    SignOut,
    /// Search signs in with this personal app's client ID (empty: none).
    SetSearchApp(String),
    SetLevelling(bool),
    /// Saves this Last.fm API account (key, shared secret) and approves it
    /// in the browser.
    ConnectLastFm(String, String),
    DisconnectLastFm,
    /// The audio cache on or off, from the next start.
    SetAudioCache(bool),
    /// The audio cache's limit in MB, from the next start.
    SetAudioCacheSize(u64),
    MeasureStorage,
    /// The system has a network again (Android tells).
    NetworkBack,
    ClearCache(Store),
    ListOutputs,
    /// Plays to this output (None: the system's default).
    SetOutput(Option<String>),
    /// Appends a track, album or artist link's songs to the target playlist.
    AddLink(String),
    Play,
    TogglePause,
    Stop,
    /// Stops, keeping where the song was, so Play goes on from there: for
    /// letting the player go when the app is put away (Android).
    Release,
    Next,
    Previous,
    Seek(u32),
    /// 0–100, as the slider shows it.
    Volume(u8),
    SetShuffle(bool),
    SetRepeat(bool),
    /// Double-click or Enter on a playlist's row.
    PlayEntry(PlaylistId, EntryId),
    Remove(PlaylistId, Vec<EntryId>),
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

    /// Shows or hides the playlists below the player (PL).
    TogglePlaylists,

    // The catalog
    ToggleBrowse,
    CloseBrowse,
    Back,
    Search(String),
    /// Drops a search from the recent ones, or all of them with None.
    ForgetSearch(Option<String>),
    OpenArtist(ArtistRef),
    /// Shows or hides an album's songs, reading them if needed.
    ToggleAlbum(String),
    /// Shows these albums' songs, or hides them all with an empty list.
    ExpandAlbums(Vec<String>),
    SetTab(ReleaseTab),
    /// Shows artists like this one (the artist page's), reading them from
    /// Last.fm if needed.
    ShowSimilar(ArtistRef),
    /// Opens a similar artist, by name, finding it on Spotify first.
    OpenSimilar(String),
    ToggleSort,
    SetTarget(PlaylistId),
    ToggleCovers,
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

    // The Spotify tab
    /// Takes the account's playback over from the device playing it.
    PlaySpotifyHere,
    /// Copies songs to the end of a playlist (the Spotify tab's: queues
    /// them).
    CopyTo(PlaylistId, Vec<TrackRef>),
}
