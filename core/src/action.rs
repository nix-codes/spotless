//! What an interface asks for. It only emits these; the controller applies
//! them. A desktop keeps actions of its own beside them (Spotless's: quitting,
//! the tray, skins, covers).

use crate::backend::Store;
use crate::model::catalog::{ReleaseTab, SpotifyTarget};
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SignIn,
    SignOut,
    /// Search signs in with this personal app's client ID (empty: none).
    SetSearchApp(String),
    SetLevelling(bool),
    /// Draws the lists and the display in this colour.
    SetPhosphor(crate::phosphor::Phosphor),
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
    /// Stops once the playing song ends, or doesn't any more (Winamp's
    /// Stop after current).
    ToggleStopAfter,
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
    /// Empties the Spotify tab: stops, and drops the song, the queue and
    /// the album or playlist it plays from. Only while no other device
    /// holds the queue.
    ClearSpotify,
    /// Takes the queue from the device holding it, paused, so that the
    /// Spotify tab can change it.
    TakeOverSpotify,
    /// Opens the Spotify tab again, closed with ClosePlaylist like any tab:
    /// Spotless joins Spotify Connect again.
    OpenSpotifyTab,
    /// The Spotify tab can be used, or not (Settings). Allowed again, it
    /// opens.
    SetSpotifyTabEnabled(bool),
    /// Moves these rows, keeping their order, to just before the row now at
    /// this index.
    Move(Vec<EntryId>, usize),
    ToggleMarks(Vec<EntryId>),

    // Playlists (tabs)
    SelectPlaylist(PlaylistId),
    NewPlaylist,
    /// Makes a playlist of these songs and shows it.
    NewPlaylistWith(Vec<TrackRef>),
    /// Deletes a playlist and its songs (never Default).
    ClosePlaylist(PlaylistId),
    RenamePlaylist(PlaylistId, String),
    /// Moves a tab to this index, counted after it is taken out.
    MovePlaylist(PlaylistId, usize),

    /// Shows or hides the playlists below the player (PL).
    TogglePlaylists,
    /// Shows or hides the equaliser below the player (EQ).
    ToggleEqualiser,
    /// The equaliser's switch, preamp and bands, as they are now.
    SetEqualiser(crate::playback::equaliser::Equaliser),
    /// Saves the equaliser's bands as a preset by this name, in place of
    /// one by the same name.
    SaveEqualiserPreset(String),
    DeleteEqualiserPreset(String),
    /// Shows or hides the deck below the player (VIS).
    ToggleDeck,
    /// What the deck plays the music on.
    SetDeckMedia(crate::store::DeckMedia),

    // The catalog
    ToggleBrowse,
    CloseBrowse,
    Back,
    Search(String),
    /// Drops a search from the recent ones, or all of them with None.
    ForgetSearch(Option<String>),
    OpenArtist(ArtistRef),
    /// Shows or hides an album's songs, reading them if needed. Spotify
    /// playlists and Liked Songs go by their URI as albums do, here and
    /// in the album actions below.
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
    /// Reads the account's Spotify playlists again (Browse does on
    /// opening), for choosing one to copy songs to.
    ReadSpotifyPlaylists,
    /// Copies songs to a Spotify playlist of the account's, or to a new
    /// one, once: it isn't kept in step with the songs' tab after.
    CopyToSpotify(SpotifyTarget, Vec<TrackRef>),

    // The Spotify tab
    /// Copies songs to the end of a playlist (the Spotify tab's: queues
    /// them).
    CopyTo(PlaylistId, Vec<TrackRef>),
    /// Moves these songs of a playlist to the end of another. To the
    /// Spotify tab: queues them, leaving them where they were if Spotify
    /// won't take them. From it: only songs to come, unless another device
    /// has the queue.
    MoveTo(PlaylistId, PlaylistId, Vec<EntryId>),
}
