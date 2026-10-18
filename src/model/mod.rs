//! The playlists and the rules for moving through them (docs/PLAN.md,
//! section 5). Pure data: no Spotify, no audio, no UI.

pub mod library;
pub mod playback;
pub mod playlist;

pub use library::Library;
pub use playback::{Playback, Step};
pub use playlist::{AlbumRef, ArtistRef, Entry, EntryId, Playlist, PlaylistId, TrackRef};
