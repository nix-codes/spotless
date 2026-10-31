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
pub use playlist::{
    AlbumRef, ArtistRef, Entry, EntryId, Grouping, Playlist, PlaylistId, Run, Source, TrackRef,
    from_source,
};
