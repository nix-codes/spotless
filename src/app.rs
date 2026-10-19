//! The application: owns the state, draws the views, then applies their
//! actions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SearchState, SessionState};
use crate::browse::{Browse, Cover, Loadable, Page};
use crate::media::{MediaEvent, Mpris, NowPlaying};
use crate::model::{Library, Playback, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status};
use crate::playback::transport::{Request, Transport};
use crate::store::{self, Settings, Tabs};
use crate::ui::{self, Inputs, View, theme::Palette};

/// Changes are saved this long after the last one.
const SAVE_DELAY: Duration = Duration::from_secs(1);

pub struct App {
    paths: Paths,
    backend: Option<Backend>,
    palette: Palette,
    session: SessionState,
    player: PlayerSnapshot,
    error: Option<String>,
    notice: Option<String>,
    inputs: Inputs,
    library: Library,
    transport: Transport,
    /// The playlist shown in the editor.
    viewed: PlaylistId,
    settings: Settings,
    /// When the first unsaved change was made.
    unsaved: Option<Instant>,
    browse: Browse,
    search: SearchState,
    media: Mpris,
    media_events: std::sync::mpsc::Receiver<MediaEvent>,
    /// Albums whose songs go to a playlist (at an index, or at the end)
    /// once they are read.
    pending_adds: Vec<(String, PlaylistId, Option<usize>)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> Self {
        let palette = Palette::CLASSIC;
        palette.apply(&cc.egui_ctx);
        let settings = Settings::load(&paths);
        let (library, tabs) = store::load_library(&paths);
        let covers = settings.covers;
        let viewed = tabs.viewed.or(tabs.playing).unwrap_or(PlaylistId::DEFAULT);
        let mut playback = Playback::new(seed());
        playback.repeat = settings.repeat;
        playback.set_shuffle(settings.shuffle);

        let (media_sender, media_events) = std::sync::mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let media = Mpris::start(Arc::new(move |event| {
            let _ = media_sender.send(event);
            ctx.request_repaint();
        }));

        let volume = volume_level(settings.volume);
        let (backend, session) = match Backend::start(paths.clone(), volume, cc.egui_ctx.clone()) {
            Ok(backend) => (Some(backend), SessionState::Connecting),
            Err(error) => (None, SessionState::Failed(format!("{error:#}"))),
        };
        Self {
            backend,
            palette,
            session,
            player: PlayerSnapshot::default(),
            error: None,
            notice: None,
            inputs: Inputs {
                volume: settings.volume,
                ..Inputs::default()
            },
            library,
            transport: Transport::new(playback),
            viewed,
            settings,
            unsaved: None,
            browse: Browse::new(viewed, covers),
            search: SearchState::SignedOut,
            pending_adds: Vec::new(),
            media,
            media_events,
            paths,
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let Some(backend) = &self.backend else { return };
        let events: Vec<Event> = backend.events.try_iter().collect();
        for event in events {
            match event {
                Event::Session(state) => {
                    self.error = None;
                    if !matches!(state, SessionState::Ready { .. }) {
                        // The engine is gone with the session.
                        self.transport.stop(&mut self.library);
                        self.player = PlayerSnapshot::default();
                    }
                    self.session = state;
                }
                Event::Player(snapshot) => {
                    if snapshot.error.is_some() {
                        self.error = snapshot.error.clone();
                    }
                    self.player = snapshot;
                }
                Event::Followed(entry) => {
                    let requests = self.transport.followed(&mut self.library, entry);
                    self.changed(requests);
                }
                Event::Ended => {
                    let requests = self.transport.ended(&mut self.library);
                    self.changed(requests);
                }
                Event::Unavailable => {
                    let requests = self.transport.unavailable(&mut self.library);
                    self.changed(requests);
                }
                Event::Tracks { playlist, tracks } => self.insert_tracks(playlist, None, tracks),
                Event::Search(state) => self.search = state,
                Event::Cover { url, result } => {
                    let cover = match result {
                        Ok(image) => Cover::Ready(ctx.load_texture(
                            &url,
                            image,
                            egui::TextureOptions::LINEAR,
                        )),
                        Err(_) => Cover::Failed,
                    };
                    self.browse.covers.insert(url, cover);
                }
                Event::SearchResults { query, results } => {
                    let loaded = match results {
                        Ok(results) => Loadable::Ready(results),
                        Err(error) => Loadable::Failed(error),
                    };
                    self.browse.searches.insert(query, loaded);
                }
                Event::Discography { artist, result } => {
                    let loaded = match result {
                        Ok(discography) => {
                            self.browse.visited(discography.artist.clone());
                            Loadable::Ready(discography)
                        }
                        Err(error) => Loadable::Failed(error),
                    };
                    self.browse.artists.insert(artist, loaded);
                }
                Event::Album { album, result } => {
                    let pending: Vec<(PlaylistId, Option<usize>)> = self
                        .pending_adds
                        .iter()
                        .filter(|(uri, _, _)| *uri == album)
                        .map(|(_, list, at)| (*list, *at))
                        .collect();
                    self.pending_adds.retain(|(uri, _, _)| *uri != album);
                    match result {
                        Ok(tracks) => {
                            for (list, at) in pending {
                                self.insert_tracks(list, at, tracks.clone());
                            }
                            self.browse.albums.insert(album, Loadable::Ready(tracks));
                        }
                        Err(error) => {
                            if !pending.is_empty() {
                                self.error = Some(error.clone());
                            }
                            self.browse.albums.insert(album, Loadable::Failed(error));
                        }
                    }
                }
                Event::Error(message) => self.error = Some(message),
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let paused = self.player.status == Status::Paused;
        let position = self.player.position_now();
        let library = &mut self.library;
        let viewed = self.viewed;
        let requests = match action {
            Action::SignIn => return self.send(Command::SignIn),
            Action::SignOut => return self.send(Command::SignOut),
            Action::SetSearchApp(client_id) => {
                let client_id = Some(client_id.trim().to_string()).filter(|id| !id.is_empty());
                self.error = None;
                self.notice = Some(match (&client_id, &self.session) {
                    (None, _) => "Search's app removed.".into(),
                    (Some(_), SessionState::Ready { .. }) => {
                        "Saved. Approve search in the browser page that opens.".into()
                    }
                    (Some(_), _) => "Saved. Search is approved when you sign in.".into(),
                });
                self.settings.set_web_client_id(client_id.clone());
                self.mark_unsaved();
                return self.send(Command::SearchApp(client_id));
            }
            Action::AddLink(text) => {
                self.error = None;
                self.notice = Some("Reading the link…".into());
                let playlist = if self.browse.open {
                    self.browse.target
                } else {
                    viewed
                };
                return self.send(Command::AddLink { text, playlist });
            }
            Action::SelectPlaylist(list) => {
                if library.get(list).is_some() {
                    self.viewed = list;
                    self.mark_unsaved();
                }
                return;
            }
            Action::NewPlaylist => {
                let name = library.fresh_name();
                let list = library.create(name.clone());
                self.viewed = list;
                self.inputs.renaming = Some((list, name));
                self.mark_unsaved();
                return;
            }
            Action::ClosePlaylist(list) => {
                let index = library.playlists().iter().position(|l| l.id == list);
                let requests = self.transport.deleting(list);
                if !self.library.delete(list) {
                    return;
                }
                // The tab before it takes its place.
                let neighbour = index
                    .and_then(|i| self.library.playlists().get(i.saturating_sub(1)))
                    .map_or(PlaylistId::DEFAULT, |l| l.id);
                if self.viewed == list {
                    self.viewed = neighbour;
                }
                if self.browse.target == list {
                    self.browse.target = self.viewed;
                }
                self.pending_adds.retain(|(_, target, _)| *target != list);
                requests
            }
            Action::RenamePlaylist(list, name) => {
                library.rename(list, &name);
                self.mark_unsaved();
                return;
            }
            Action::MovePlaylist(list, index) => {
                library.move_playlist(list, index);
                self.mark_unsaved();
                return;
            }
            Action::Seek(ms) => return self.send(Command::Seek(ms)),
            Action::Volume(volume) => {
                self.settings.volume = volume;
                self.mark_unsaved();
                return self.send(Command::Volume(volume_level(volume)));
            }
            Action::Play => self.transport.play(library, viewed, paused, position),
            Action::TogglePause => self.transport.toggle_pause(paused),
            Action::Stop => self.transport.stop(library),
            Action::Next => self.transport.next(library, viewed),
            Action::Previous => self.transport.prev(library, viewed, position),
            Action::SetShuffle(on) => {
                self.settings.shuffle = on;
                self.transport.set_shuffle(library, on)
            }
            Action::SetRepeat(on) => {
                self.settings.repeat = on;
                self.transport.set_repeat(library, on)
            }
            Action::PlayEntry(list, entry) => {
                self.error = None;
                self.transport.play_entry(library, list, entry, position)
            }
            Action::Remove(list, entries) => {
                if let Some(playlist) = library.get_mut(list) {
                    playlist.remove(&entries);
                }
                self.transport.edited(library, list)
            }
            Action::Move(entries, index) => {
                if let Some(list) = library.get_mut(viewed) {
                    list.move_to(&entries, index);
                }
                self.transport.edited(library, viewed)
            }
            Action::ToggleMarks(entries) => {
                if let Some(list) = library.get_mut(viewed) {
                    for entry in entries {
                        list.toggle_mark(entry);
                    }
                }
                self.transport.edited(library, viewed)
            }
            Action::PlayNow(track) => self.play_now(track, position),
            browse => return self.apply_browse(browse),
        };
        self.changed(requests);
    }

    fn apply_browse(&mut self, action: Action) {
        match action {
            Action::ToggleBrowse => {
                self.browse.open = !self.browse.open;
                if self.browse.open {
                    self.browse.target = self.viewed;
                }
            }
            Action::CloseBrowse => self.browse.open = false,
            Action::Back => self.browse.back(),
            Action::Search(query) => {
                let query = query.trim().to_string();
                if query.is_empty() {
                    return;
                }
                // A pasted Spotify link adds its songs instead.
                if crate::spotify::catalog::parse_uri(&query).is_ok() {
                    self.inputs.query.clear();
                    return self.apply(Action::AddLink(query));
                }
                self.browse.go(Page::Search(query.clone()));
                let cached = matches!(self.browse.searches.get(&query), Some(Loadable::Ready(_)));
                // Unapproved search asks for its approval first, by itself.
                if !cached && self.search != SearchState::NoApp {
                    self.browse
                        .searches
                        .insert(query.clone(), Loadable::Loading);
                    self.send(Command::Search(query));
                }
            }
            Action::OpenArtist(artist) => {
                self.browse.go(Page::Artist(artist.uri.clone()));
                match self.browse.artists.get(&artist.uri) {
                    Some(Loadable::Ready(_) | Loadable::Loading) => self.browse.visited(artist),
                    _ => {
                        self.browse
                            .artists
                            .insert(artist.uri.clone(), Loadable::Loading);
                        self.send(Command::Discography(artist.uri));
                    }
                }
            }
            Action::ToggleAlbum(album) => {
                if !self.browse.expanded.remove(&album) {
                    self.browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::ExpandAlbums(albums) => {
                if albums.is_empty() {
                    self.browse.expanded.clear();
                }
                for album in albums {
                    self.browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::SetTab(tab) => self.browse.tab = tab,
            Action::ToggleSort => self.browse.newest_first = !self.browse.newest_first,
            Action::SetTarget(list) => self.browse.target = list,
            Action::ToggleCovers => {
                self.browse.show_covers = !self.browse.show_covers;
                self.settings.covers = self.browse.show_covers;
                self.mark_unsaved();
            }
            Action::LoadCover(url) => {
                if !self.browse.covers.contains_key(&url) {
                    self.browse.covers.insert(url.clone(), Cover::Loading);
                    self.send(Command::Cover(url));
                }
            }
            Action::AddTracks(tracks) => self.insert_tracks(self.browse.target, None, tracks),
            Action::DropTracks(tracks, at) => {
                self.insert_tracks(self.browse.target, Some(at), tracks)
            }
            Action::AddAlbum(album) => self.add_album(album, None),
            Action::DropAlbum(album, at) => self.add_album(album, Some(at)),
            _ => {}
        }
    }

    fn add_album(&mut self, album: String, at: Option<usize>) {
        let target = self.browse.target;
        match self.browse.albums.get(&album) {
            Some(Loadable::Ready(tracks)) => {
                let tracks = tracks.clone();
                self.insert_tracks(target, at, tracks);
            }
            _ => {
                self.pending_adds.push((album.clone(), target, at));
                self.notice = Some("Reading the album…".into());
                self.read_album(album);
            }
        }
    }

    /// Reads an album's songs unless they are read or being read.
    fn read_album(&mut self, album: String) {
        if matches!(
            self.browse.albums.get(&album),
            Some(Loadable::Ready(_) | Loadable::Loading)
        ) {
            return;
        }
        self.browse.albums.insert(album.clone(), Loadable::Loading);
        self.send(Command::Album(album));
    }

    /// Adds songs to a playlist before the row at `at`, or at the end.
    fn insert_tracks(&mut self, playlist: PlaylistId, at: Option<usize>, tracks: Vec<TrackRef>) {
        let entries: Vec<_> = tracks
            .into_iter()
            .map(|track| self.library.entry(track))
            .collect();
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        let Some(list) = self.library.get_mut(playlist) else {
            return;
        };
        list.insert(at.unwrap_or(usize::MAX), entries);
        self.notice = Some(match ids.len() {
            1 => format!("Added 1 song to {}", list.name),
            n => format!("Added {n} songs to {}", list.name),
        });
        self.browse.flash(ids);
        let requests = self.transport.edited(&mut self.library, playlist);
        self.changed(requests);
    }

    /// Inserts `track` after the target playlist's current song and plays it.
    fn play_now(&mut self, track: TrackRef, position: u32) -> Vec<Request> {
        let target = self.browse.target;
        let entry = self.library.entry(track);
        let id = entry.id;
        let Some(list) = self.library.get_mut(target) else {
            return Vec::new();
        };
        let at = list
            .cursor()
            .and_then(|cursor| list.index_of(cursor))
            .map_or(list.len(), |i| i + 1);
        list.insert(at, vec![entry]);
        self.browse.flash([id]);
        self.error = None;
        let mut requests = self.transport.edited(&mut self.library, target);
        requests.extend(
            self.transport
                .play_entry(&mut self.library, target, id, position),
        );
        requests
    }

    /// The desktop's media keys and controls, as the buttons would do.
    fn media_actions(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let status = self.player.status;
        let position = i64::from(self.player.position_now());
        let length = i64::from(self.player.track.as_ref().map_or(0, |t| t.duration_ms));
        let mut actions = Vec::new();
        for event in self.media_events.try_iter() {
            match event {
                MediaEvent::Play => match status {
                    Status::Paused => actions.push(Action::TogglePause),
                    Status::Stopped => actions.push(Action::Play),
                    _ => {}
                },
                MediaEvent::Pause if status == Status::Playing => actions.push(Action::TogglePause),
                MediaEvent::Pause => {}
                MediaEvent::PlayPause if status == Status::Stopped => actions.push(Action::Play),
                MediaEvent::PlayPause => actions.push(Action::TogglePause),
                MediaEvent::Stop => actions.push(Action::Stop),
                MediaEvent::Next => actions.push(Action::Next),
                MediaEvent::Previous => actions.push(Action::Previous),
                MediaEvent::SeekBy(us) => {
                    let to = (position + us / 1000).clamp(0, length.max(0));
                    actions.push(Action::Seek(to as u32));
                }
                MediaEvent::SeekTo(us) => {
                    actions.push(Action::Seek((us / 1000).clamp(0, length.max(0)) as u32))
                }
                MediaEvent::Volume(level) => {
                    let percent = (level * 100.0).round() as u8;
                    self.inputs.volume = percent;
                    actions.push(Action::Volume(percent));
                }
                MediaEvent::Raise => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    // Where the desktop refuses focus (Wayland), the window
                    // at least asks for attention.
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                MediaEvent::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        actions
    }

    /// The model changed: send what the engine must do, and save soon.
    fn changed(&mut self, requests: Vec<Request>) {
        for request in requests {
            self.send(Command::Player(request));
        }
        self.mark_unsaved();
    }

    fn mark_unsaved(&mut self) {
        self.unsaved.get_or_insert_with(Instant::now);
    }

    fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    fn save(&mut self) {
        self.unsaved = None;
        self.transport
            .remember_position(&mut self.library, self.player.position_now());
        let tabs = Tabs {
            playing: self.transport.playback.playing(),
            viewed: Some(self.viewed),
        };
        if let Err(error) = store::save_library(&self.paths, &self.library, tabs) {
            log::error!("{error:#}");
            self.error = Some("Couldn't save the playlists.".into());
        }
        if let Err(error) = self.settings.save(&self.paths) {
            log::error!("{error:#}");
        }
    }
}

/// The slider's 0–100 as librespot's 0–65535.
fn volume_level(percent: u8) -> u16 {
    (u32::from(percent.min(100)) * u32::from(u16::MAX) / 100) as u16
}

/// A different shuffle every run.
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive(&ui.ctx().clone());
        let mut actions = Vec::new();
        let web_client_id = self.settings.web_client_id();
        let playlist = self
            .library
            .get(self.viewed)
            .or_else(|| self.library.get(PlaylistId::DEFAULT))
            .expect("Default always exists");
        let view = View {
            palette: self.palette,
            session: &self.session,
            player: &self.player,
            error: self.error.as_deref(),
            notice: self.notice.as_deref(),
            playlist,
            shuffle: self.transport.playback.shuffle(),
            repeat: self.transport.playback.repeat,
            library: &self.library,
            playing_list: self.transport.playing_list(),
            browse: &self.browse,
            search: &self.search,
            web_client_id: web_client_id.as_deref(),
        };
        ui::draw(ui, &view, &mut self.inputs, &mut actions);
        actions.extend(self.media_actions(&ui.ctx().clone()));
        for action in actions {
            self.apply(action);
        }
        self.media
            .update(NowPlaying::from_player(&self.player, self.settings.volume));
        // The clock moves on its own while a song plays.
        if self.player.status == Status::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if let Some(since) = self.unsaved {
            match SAVE_DELAY.checked_sub(since.elapsed()) {
                Some(wait) if !wait.is_zero() => ui.ctx().request_repaint_after(wait),
                _ => self.save(),
            }
        }
    }

    /// Tab and Shift+Tab never walk egui's focus through the buttons: they
    /// move between Browse's areas (`Inputs::tab`). Ctrl+Tab (switching
    /// playlists) still gets through.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let tab = &mut self.inputs.tab;
        raw_input.events.retain(|event| match event {
            egui::Event::Key {
                key: egui::Key::Tab,
                modifiers,
                pressed,
                ..
            } if !modifiers.command && !modifiers.ctrl => {
                if *pressed {
                    *tab += if modifiers.shift { -1 } else { 1 };
                }
                false
            }
            _ => true,
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.desk.to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::volume_level;

    #[test]
    fn volume_spans_librespots_range() {
        assert_eq!(volume_level(0), 0);
        assert_eq!(volume_level(100), u16::MAX);
        assert_eq!(volume_level(250), u16::MAX);
    }
}
