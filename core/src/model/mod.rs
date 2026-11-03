//! The playlists and the rules for moving through them. Pure data: no
//! Spotify, no audio, no UI.

pub mod catalog;
internal! {
    mod library;
    mod playback;
    mod playlist;
    mod sync;
}

pub use library::Library;
pub use playback::{Playback, Step};
pub(crate) use playlist::Snapshot;
pub use playlist::{
    AlbumPlace, AlbumRef, ArtistRef, Entry, EntryId, Grouping, Order, Playlist, PlaylistId, Run,
    SortHeaders, Source, TrackRef, from_source,
};
