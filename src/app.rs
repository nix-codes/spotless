//! The desktop application: the window, the tray, media keys and skins
//! around the shared [`Controller`]. It draws the views, then applies their
//! actions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::action::{Action, Actions, AnyAction, Desktop};
use crate::backend::{Command, Event};
use crate::controller::Controller;
use crate::focus::Focus;
use crate::media::{EventHook, MediaControls, MediaEvent, NowPlaying};
use crate::model::PlaylistId;
use crate::paths::Paths;
use crate::phosphor::Phosphor;
use crate::playback::engine::Status;
use crate::skin::format;
use crate::store;
use crate::tray::{Tray, TrayState};
use crate::ui::skin_paint::SkinPaint;
use crate::ui::{
    self, Cover, Inputs, View,
    theme::{self, Palette},
};
use crate::update::{self, Updater};

pub struct App {
    controller: Controller,
    palette: Palette,
    ctx: egui::Context,
    /// The skin's pieces, when it has any.
    art: Option<Arc<SkinPaint>>,
    /// What the chosen skin lacks, or got wrong.
    skin_note: Option<String>,
    /// Why the chosen skin couldn't be read.
    skin_error: Option<String>,
    inputs: Inputs,
    /// The album covers fetched or being fetched, by image URL.
    covers: HashMap<String, Cover>,
    media: MediaControls,
    media_events: std::sync::mpsc::Receiver<MediaEvent>,
    /// Where the media keys and the tray send their events.
    media_hook: EventHook,
    tray: Option<Tray>,
    /// The window is hidden in the tray.
    window_hidden: bool,
    /// Brings the window to the front, past the window manager.
    focus: Focus,
    /// Quitting for real: closing the window doesn't hide it.
    quitting: bool,
    updater: Updater,
    /// The updated program to start once this one has quit.
    restart: Option<PathBuf>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> Self {
        let controller = Controller::new(paths, {
            let ctx = cc.egui_ctx.clone();
            move || ctx.request_repaint()
        });
        let settings = &controller.state().settings;
        let palette = Palette::CLASSIC;
        palette.apply(&cc.egui_ctx);
        ui::deck::install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(settings.zoom);
        // Maximised from the start, the window manager would not take it.
        if settings.window.is_some_and(|place| place.maximized) {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }

        let (media_sender, media_events) = std::sync::mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let media_hook: EventHook = Arc::new(move |event| {
            let _ = media_sender.send(event);
            ctx.request_repaint();
        });
        let media = MediaControls::start(cc, Arc::clone(&media_hook));
        let tray = if settings.tray {
            Tray::start(Arc::clone(&media_hook))
        } else {
            None
        };
        let inputs = Inputs {
            volume: settings.volume,
            ..Inputs::default()
        };
        let mut app = Self {
            controller,
            palette,
            ctx: cc.egui_ctx.clone(),
            art: None,
            skin_note: None,
            skin_error: None,
            inputs,
            covers: HashMap::new(),
            media,
            media_events,
            media_hook,
            tray,
            window_hidden: false,
            focus: Focus::new(cc),
            quitting: false,
            updater: Updater::new({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            restart: None,
        };
        // A skin from the skins folder of an older Spotless is gone:
        // Spotless Classic takes its place.
        let skin = app.controller.state().settings.skin.clone();
        let skin = match skin.as_deref() {
            None | Some(format::built_in::WINAMP_CLASSIC_KEY) => skin,
            Some(_) => {
                app.controller.settings_mut().skin = None;
                None
            }
        };
        if let Err(error) = app.use_skin(skin.as_deref()) {
            app.skin_error = Some(error);
        }
        // Opened from a terminal or a launcher, the window comes to the front.
        app.focus.want(&cc.egui_ctx);
        app
    }

    /// Takes in the backend's news; covers become textures here.
    fn receive(&mut self, ctx: &egui::Context) {
        for event in self.controller.receive() {
            if let Event::Cover { url, result } = event {
                let cover = match result {
                    Ok(image) => Cover::Ready(ctx.load_texture(
                        &url,
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width as usize, image.height as usize],
                            &image.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    )),
                    Err(_) => Cover::Failed,
                };
                self.covers.insert(url, cover);
            }
        }
    }

    /// Applies an action: the desktop's own here, the rest through the
    /// controller, some with a step of the desktop's around them.
    fn apply(&mut self, action: AnyAction) {
        let action = match action {
            AnyAction::Desktop(action) => return self.apply_desktop(action),
            AnyAction::Core(action) => action,
        };
        match action {
            Action::NewPlaylist => {
                self.controller.apply(action);
                // Its name is ready to be typed over.
                let state = self.controller.state();
                if let Some(list) = state.library.get(state.viewed) {
                    self.inputs.renaming = Some((list.id, list.name.clone()));
                }
            }
            Action::Search(ref query)
                if crate::spotify::catalog::parse_uri(query.trim()).is_ok() =>
            {
                // A pasted link adds its songs; the field is left empty.
                self.inputs.query.clear();
                self.controller.apply(action);
            }
            Action::Search(ref query) => {
                // A recent search fills the field, as if typed.
                self.inputs.query = query.trim().to_string();
                self.controller.apply(action);
            }
            action => self.controller.apply(action),
        }
    }

    fn apply_desktop(&mut self, action: Desktop) {
        match action {
            Desktop::Quit => self.quitting = true,
            Desktop::SetTray(on) => {
                self.controller.settings_mut().tray = on;
                self.tray = if on {
                    self.tray
                        .take()
                        .or_else(|| Tray::start(Arc::clone(&self.media_hook)))
                } else {
                    None
                };
            }
            Desktop::SetSkin(skin) => match self.use_skin(skin.as_deref()) {
                Ok(()) => {
                    self.skin_error = None;
                    self.controller.settings_mut().skin = skin;
                }
                Err(error) => self.skin_error = Some(error),
            },
            Desktop::SetPhosphor(phosphor) => {
                self.controller.apply(Action::SetPhosphor(phosphor));
                let skin = self.controller.state().settings.skin.clone();
                if let Err(error) = self.use_skin(skin.as_deref()) {
                    self.skin_error = Some(error);
                }
            }
            Desktop::SetUpdates(on) => self.controller.settings_mut().updates = on,
            Desktop::SetUpdateNotices(on) => self.controller.settings_mut().update_notices = on,
            Desktop::CheckForUpdate => self.updater.check(),
            Desktop::Update => self.updater.install(),
            Desktop::Restart => {
                if let Some(program) = self.updater.restart_into() {
                    self.restart = Some(program.to_path_buf());
                    self.quitting = true;
                }
            }
            Desktop::PickSpotifyPlaylist { tracks, name, new } => {
                self.inputs.spotify_pick = Some(if new {
                    crate::ui::Picker::naming(tracks, name)
                } else {
                    self.controller.apply(Action::ReadSpotifyPlaylists);
                    crate::ui::Picker::new(tracks, name)
                });
            }
            Desktop::CopyLinks(uris) => {
                let links = crate::spotify::links(uris.iter().map(String::as_str));
                let notice = match links.lines().count() {
                    0 => "Local files have no link to share.".to_string(),
                    1 => "Copied the link.".to_string(),
                    n => format!("Copied {n} links, one a line."),
                };
                if !links.is_empty() {
                    self.ctx.copy_text(links);
                }
                self.controller.tell(notice);
            }
            Desktop::LoadCover(url) => {
                if !self.covers.contains_key(&url) {
                    self.covers.insert(url.clone(), Cover::Loading);
                    self.controller.send(Command::Cover(url));
                }
            }
        }
    }

    /// The desktop's media keys and controls, as the buttons would do.
    fn media_actions(&mut self, ctx: &egui::Context) -> Actions {
        let player = self.controller.state().now_playing();
        let status = player.status;
        let position = i64::from(player.position_now());
        let length = i64::from(player.track.as_ref().map_or(0, |t| t.duration_ms));
        let mut actions = Actions::default();
        let events: Vec<MediaEvent> = self.media_events.try_iter().collect();
        for event in events {
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
                MediaEvent::Raise => self.show_window(ctx),
                MediaEvent::Quit => actions.push(Desktop::Quit),
                MediaEvent::ToggleWindow if self.window_hidden => self.show_window(ctx),
                MediaEvent::ToggleWindow => self.hide_window(ctx),
            }
        }
        actions
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.window_hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.focus.want(ctx);
        // Where the desktop refuses focus (Wayland), the window at least
        // asks for attention.
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
    }

    /// Hides the window in the tray, if a panel shows the tray icon: a
    /// window hidden anywhere else couldn't be found again.
    fn hide_window(&mut self, ctx: &egui::Context) {
        if self.tray.as_ref().is_some_and(Tray::shown) {
            self.window_hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    /// Closing the window hides it in the tray, unless quitting.
    fn keep_in_tray(&mut self, ctx: &egui::Context) {
        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing && !self.quitting && self.tray.as_ref().is_some_and(Tray::shown) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
    }

    /// Saves the zoom when it changes, by the settings or by egui's own
    /// Ctrl+plus and Ctrl+minus, within the range the settings allow.
    fn keep_zoom(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let kept = zoom.clamp(*store::ZOOM_RANGE.start(), *store::ZOOM_RANGE.end());
        if kept != zoom {
            ctx.set_zoom_factor(kept);
        }
        if (kept - self.controller.state().settings.zoom).abs() > 0.001 {
            self.controller.settings_mut().zoom = kept;
        }
    }

    /// Keeps where the window is and how big, to open it there next time.
    /// A maximised window keeps the size it had before.
    fn keep_window_place(&mut self, ctx: &egui::Context) {
        let (outer, minimized, maximized) = ctx.input(|i| {
            let viewport = i.viewport();
            (
                viewport.outer_rect,
                viewport.minimized.unwrap_or(false),
                viewport.maximized.unwrap_or(false),
            )
        });
        if minimized {
            return;
        }
        let zoom = ctx.zoom_factor();
        let kept = self.controller.state().settings.window;
        let size = ctx.content_rect().size();
        let place = store::WindowPlace {
            position: match (maximized, kept) {
                (true, Some(kept)) => kept.position,
                _ => outer.map(|outer| (outer.min.to_vec2() * zoom).into()),
            },
            size: match (maximized, kept) {
                (true, Some(kept)) => kept.size,
                _ => [size.x * zoom, self.inputs.fit.with_playlists(size.y) * zoom],
            },
            maximized,
        };
        if kept != Some(place) {
            self.controller.settings_mut().window = Some(place);
        }
    }

    /// Draws with Winamp Classic, or Spotless Classic for anything else.
    /// A skin that can't be read leaves things as they are.
    fn use_skin(&mut self, skin: Option<&str>) -> Result<(), String> {
        let phosphor = self.controller.state().settings.phosphor;
        let look = use_skin(&self.ctx, skin, phosphor)?;
        self.palette = look.palette;
        self.art = look.art;
        self.skin_note = look.note;
        Ok(())
    }
}

/// A skin in use: its colours, its pieces and what it lacks or got wrong.
pub struct Look {
    pub palette: Palette,
    pub art: Option<Arc<SkinPaint>>,
    pub note: Option<String>,
}

/// Draws `ctx` with Winamp Classic, or Spotless Classic for anything else,
/// in `phosphor`'s colours. A skin that can't be read leaves things as
/// they are.
pub fn use_skin(
    ctx: &egui::Context,
    skin: Option<&str>,
    phosphor: Phosphor,
) -> Result<Look, String> {
    let (name, loaded) = match skin {
        Some(format::built_in::WINAMP_CLASSIC_KEY) => {
            ("Winamp Classic", format::built_in::winamp_classic())
        }
        _ => ("Spotless Classic", format::built_in::spotless_classic()),
    };
    let mut loaded = loaded.map_err(|error| {
        log::warn!("skin {name}: {error}");
        format!("{name}: {error}")
    })?;
    if let Some(color) = theme::display_colour(phosphor) {
        loaded.tint_display(color);
    }
    let mut notes = Vec::new();
    for problem in loaded.problems.iter().take(3) {
        log::warn!("skin: {}: {}", problem.at, problem.message);
        notes.push(format!("{}: {}.", problem.at, problem.message));
    }
    if loaded.problems.len() > 3 {
        notes.push(format!("And {} more.", loaded.problems.len() - 3));
    }
    let palette = loaded.palette.with_phosphor(phosphor);
    palette.apply(ctx);
    let has_pieces = !loaded.images.is_empty();
    let paint = SkinPaint::new(ctx, loaded);
    let missing = paint.missing_player();
    if has_pieces && !missing.is_empty() {
        notes.insert(
            0,
            format!(
                "The player is drawn as Spotless Classic's: the skin has no {}.",
                missing.join(", ")
            ),
        );
    }
    let art = has_pieces.then(|| Arc::new(paint));
    crate::ui::skin_paint::set_current(ctx, art.clone());
    Ok(Look {
        palette,
        art,
        note: (!notes.is_empty()).then(|| notes.join(" ")),
    })
}

impl eframe::App for App {
    /// Everything but drawing: it also runs while the window is hidden in
    /// the tray, whenever something asks for a repaint.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        for action in self.media_actions(ctx) {
            self.apply(action);
        }
        if self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.keep_in_tray(ctx);
        self.focus.tick(ctx);
        let state = self.controller.state();
        let player = &state.now_playing();
        self.media
            .update(NowPlaying::from_player(player, state.settings.volume));
        if let Some(tray) = &mut self.tray {
            let now_playing = player
                .track
                .as_ref()
                .map(|t| format!("{} - {}", t.artists, t.title))
                .unwrap_or_default();
            tray.update(TrayState {
                playing: player.status == Status::Playing,
                now_playing,
                window_hidden: self.window_hidden,
            });
        }
        // The clock moves on its own while a song plays.
        if player.status == Status::Playing {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some(wait) = self.controller.tick() {
            ctx.request_repaint_after(wait);
        }
        let daily = self.controller.state().settings.updates;
        if let Some(wait) = self.updater.tick(daily) {
            ctx.request_repaint_after(wait);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions = Actions::default();
        let state = self.controller.state();
        let web_client_id = state.settings.web_client_id();
        let playlist = state
            .list(state.viewed)
            .or_else(|| state.library.get(PlaylistId::DEFAULT))
            .expect("Default always exists");
        let player = state.now_playing();
        let tabs = state.tabs();
        let view = View {
            palette: self.palette,
            session: &state.session,
            player: &player,
            error: state.error.as_deref(),
            notice: state.notice.as_deref(),
            playlist,
            left_ms: state.left_ms(playlist.id),
            shuffle: state.transport.playback.shuffle(),
            repeat: state.transport.playback.repeat,
            stop_after: state.transport.stop_after(),
            library: &state.library,
            tabs: &tabs,
            playing_list: state
                .playing_list()
                .or(state.waiting().map(|(list, _)| list)),
            spotify: state.spotify.as_ref(),
            spotify_list: &state.spotify_list,
            browse: &state.browse,
            covers: &self.covers,
            search: &state.search,
            lastfm: &state.lastfm,
            web_client_id: web_client_id.as_deref(),
            settings: &state.settings,
            storage: state.storage,
            outputs: &state.outputs,
            tray_shown: self.tray.as_ref().is_some_and(Tray::shown),
            skin_error: self.skin_error.as_deref(),
            art: self.art.as_deref(),
            skin_note: self.skin_note.as_deref(),
            update: self.updater.status(),
        };
        ui::draw(ui, &view, &mut self.inputs, &mut actions);
        self.keep_zoom(ui.ctx());
        self.keep_window_place(ui.ctx());
        // What the actions change is saved and shown by the next logic pass.
        if !actions.is_empty() {
            ui.ctx().request_repaint();
        }
        for action in actions {
            self.apply(action);
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
        self.controller.save();
        if let Some(program) = self.restart.take() {
            update::restart(&program);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.desk.to_normalized_gamma_f32()
    }
}
