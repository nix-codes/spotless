//! What an interface asks for. It only emits these; the controller applies
//! them, each kind in its part of the same name (`controller/`). A desktop
//! keeps actions of its own beside them (Spotless's: quitting, the tray,
//! skins, covers).

use crate::backend::Store;
use crate::lastfm::LovedSong;
use crate::model::catalog::{ReleaseTab, SpotifyTarget};
use crate::model::{ArtistRef, EntryId, Order, PlaylistId, TrackRef};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Settings(SettingsAction),
    LastFm(LastFmAction),
    Playback(PlaybackAction),
    Playlists(PlaylistsAction),
    Sync(SyncAction),
    View(ViewAction),
    Browse(BrowseAction),
    SpotifyLists(SpotifyListsAction),
    SpotifyTab(SpotifyTabAction),
}

/// Signing in and out, search's app, and the settings that aren't what shows: audio and the lists' looks.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    SignIn,
    SignOut,
    /// Search signs in with this personal app's client ID (empty: none).
    SetSearchApp(String),
    SetLevelling(bool),
    /// Draws the lists and the display in this colour.
    SetPhosphor(crate::phosphor::Phosphor),
    /// Puts the playlists' songs under headers by this.
    SetGrouping(crate::model::playlist::Grouping),
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
}

/// Last.fm: connecting, and the songs loved there.
#[derive(Clone, Debug, PartialEq)]
pub enum LastFmAction {
    /// Saves this Last.fm API account (key, shared secret) and approves it
    /// in the browser.
    ConnectLastFm(String, String),
    DisconnectLastFm,
    /// Loves a song on Last.fm, by its first artist and title, or doesn't
    /// any more.
    SetLoved {
        artist: String,
        title: String,
        loved: bool,
    },
    /// Finds songs loved on Last.fm on Spotify and appends them to the
    /// target playlist.
    AddLoved(Vec<LovedSong>),
    /// The same, dropped on the slim column, before the row at this index.
    DropLoved(Vec<LovedSong>, usize),
    /// Finds a song loved on Last.fm on Spotify and plays it as PlayNow.
    PlayLoved(LovedSong),
}

/// The player's buttons, and songs played from a playlist or Browse.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackAction {
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
    /// Puts a song after the target playlist's current one and plays it.
    PlayNow(TrackRef),
}

/// The playlists: their songs, and the tabs.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaylistsAction {
    /// Appends a track, album or artist link's songs to the target playlist.
    AddLink(String),
    Remove(PlaylistId, Vec<EntryId>),
    /// Moves these rows, keeping their order, to just before the row now at
    /// this index.
    Move(Vec<EntryId>, usize),
    ToggleMarks(Vec<EntryId>),
    /// Sorts or shuffles a playlist, once. UNDO puts it back.
    Reorder(PlaylistId, Order),
    /// Puts back the order from before the last `Reorder`, while nothing
    /// has changed its songs since.
    UndoReorder,
    /// Switches a playlist's headers on or off.
    SetHeaders(PlaylistId, bool),

    SelectPlaylist(PlaylistId),
    NewPlaylist,
    /// Makes a playlist of these songs and shows it.
    NewPlaylistWith(Vec<TrackRef>),
    /// Deletes a playlist and its songs (never Default).
    ClosePlaylist(PlaylistId),
    RenamePlaylist(PlaylistId, String),
    /// Moves a tab to this index, counted after it is taken out.
    MovePlaylist(PlaylistId, usize),

    /// Copies songs to the end of a playlist (the Spotify tab's: queues
    /// them).
    CopyTo(PlaylistId, Vec<TrackRef>),
    /// Moves these songs of a playlist to the end of another. To the
    /// Spotify tab: queues them, leaving them where they were if Spotify
    /// won't take them. From it: only songs to come, unless another device
    /// has the queue.
    MoveTo(PlaylistId, PlaylistId, Vec<EntryId>),
}

/// Playlists kept the same on the account's other devices.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncAction {
    /// Syncs a playlist, keeping it the same on the account's other
    /// devices, or stops: its Spotify playlist is deleted, and it stays
    /// here only (the other devices keep theirs, not synced).
    SetSync(PlaylistId, bool),
    /// Syncs a playlist whose name the Spotless folder has already,
    /// deleting that one first (its devices keep it, not synced).
    ReplaceSynced(PlaylistId),
    /// The question of a name taken in the Spotless folder goes unanswered.
    DismissSyncNameTaken,
    /// Opens a playlist of the Spotless folder as a synced tab, by URI and
    /// name, or shows it if it's open.
    OpenSynced(String, String),
}

/// What shows below the player (PL, EQ, VIS) and how: the equaliser, the deck, the time.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewAction {
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
    /// What the deck measures the music on: the spectrum or VU meters.
    SetDeckMeter(crate::store::DeckMeter),
    SetVuFace(crate::store::VuFace),
    /// What loudness the VU meters read as 0 VU (CAL).
    SetVuCalibration(crate::store::VuCalibration),
    /// The cassette holds the whole playlist (MIX) or the song (SINGLE).
    SetCassetteMix(bool),
    /// The turntable plays the whole playlist on an LP (33) or a single
    /// for each song (45).
    SetVinylLp(bool),
    /// The reels play the whole playlist at 3¾ IPS, or a song a reel at
    /// 7½.
    SetReelSlow(bool),
    /// The CD player plays a mini CD for each song (8cm) or the playlist
    /// on one disc (12cm).
    SetCdMini(bool),
    /// The time shows what's left of the song, or what has played.
    ToggleTimeLeft,
}

/// Browse: searching, artists, albums, and adding their songs to the target playlist.
#[derive(Clone, Debug, PartialEq)]
pub enum BrowseAction {
    ToggleBrowse,
    CloseBrowse,
    Back,
    Search(String),
    /// Reads the search's next page of songs.
    MoreSongs(String),
    /// Shows the rest of the search's artists already read, or reads
    /// Spotify's next page of them.
    MoreArtists(String),
    /// Opens the page's find with this typed in it, or closes it with
    /// None. On an artist's page it reads the songs of the tab's releases.
    Find(Option<String>),
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
    /// The artist page shows its popular songs instead of the releases.
    ShowPopular,
    /// Opens a similar artist, by name, finding it on Spotify first.
    OpenSimilar(String),
    ToggleSort,
    SetTarget(PlaylistId),
    ToggleCovers,
    /// Appends songs to the target playlist.
    AddTracks(Vec<TrackRef>),
    /// Appends an album's songs to the target playlist.
    AddAlbum(String),
    /// Appends these albums' songs to the target playlist, in this order,
    /// all at once when every one is read.
    AddAlbums(Vec<String>),
    /// Appends what the artist's page shows to the target playlist (ADD
    /// ALL; see `Browse::add_all`).
    AddAll,
    /// Songs dropped on the slim column, before the row at this index.
    DropTracks(Vec<TrackRef>, usize),
    /// An album dropped on the slim column, before the row at this index.
    DropAlbum(String, usize),
    /// Shows this tab of Browse, opening Browse if needed.
    ShowBrowseTab(crate::browse::BrowseTab),
}

/// The account's Spotify playlists, in YOUR SPOTIFY.
#[derive(Clone, Debug, PartialEq)]
pub enum SpotifyListsAction {
    /// Shows a Spotify playlist's songs in YOUR SPOTIFY, or Liked Songs',
    /// by URI.
    OpenSpotifyPlaylist(String),
    /// Removes songs from a Spotify playlist of the account's own, or
    /// from Liked Songs (un-likes them), by their index among its songs
    /// read.
    RemoveFromSpotify(String, Vec<usize>),
    /// Takes a Spotify playlist out of the account's list, by URI:
    /// deletes it if it's the account's own, else unfollows it. Never
    /// Liked Songs.
    UnlistSpotifyPlaylist(String),
    /// Puts the songs or playlist removed last back where they were.
    UndoSpotifyRemoval,
    /// Reads the account's Spotify playlists again (Browse does on
    /// opening), for choosing one to copy songs to.
    ReadSpotifyPlaylists,
    /// Copies songs to a Spotify playlist of the account's, or to a new
    /// one, once: it isn't kept in step with the songs' tab after.
    CopyToSpotify(SpotifyTarget, Vec<TrackRef>),
}

/// The Spotify tab's own: Spotless in Spotify Connect.
#[derive(Clone, Debug, PartialEq)]
pub enum SpotifyTabAction {
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
}

impl From<SettingsAction> for Action {
    fn from(action: SettingsAction) -> Self {
        Self::Settings(action)
    }
}

impl From<LastFmAction> for Action {
    fn from(action: LastFmAction) -> Self {
        Self::LastFm(action)
    }
}

impl From<PlaybackAction> for Action {
    fn from(action: PlaybackAction) -> Self {
        Self::Playback(action)
    }
}

impl From<PlaylistsAction> for Action {
    fn from(action: PlaylistsAction) -> Self {
        Self::Playlists(action)
    }
}

impl From<SyncAction> for Action {
    fn from(action: SyncAction) -> Self {
        Self::Sync(action)
    }
}

impl From<ViewAction> for Action {
    fn from(action: ViewAction) -> Self {
        Self::View(action)
    }
}

impl From<BrowseAction> for Action {
    fn from(action: BrowseAction) -> Self {
        Self::Browse(action)
    }
}

impl From<SpotifyListsAction> for Action {
    fn from(action: SpotifyListsAction) -> Self {
        Self::SpotifyLists(action)
    }
}

impl From<SpotifyTabAction> for Action {
    fn from(action: SpotifyTabAction) -> Self {
        Self::SpotifyTab(action)
    }
}
