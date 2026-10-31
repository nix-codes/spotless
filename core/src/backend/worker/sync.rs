//! The worker's part in synced playlists: sending a sync, making a
//! playlist's Spotify playlist, and reading the Spotless folder.

use super::Worker;
use crate::backend::{Command, Event};
use crate::spotify::synced;

impl Worker {
    /// Syncs a playlist, makes one's Spotify playlist, or reads the folder.
    pub(super) fn handle_sync(&mut self, command: Command) {
        match command {
            Command::Sync { list, job } => {
                let sent = self.on_session(move |session| async move {
                    let result = synced::sync(&session, job)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    if let Err(error) = &result {
                        log::warn!("unable to sync a playlist: {error}");
                    }
                    Event::Synced { list, result }
                });
                if !sent {
                    let result = Err("offline".to_string());
                    (self.emit)(Event::Synced { list, result });
                }
            }
            Command::CreateSynced {
                list,
                name,
                replace,
            } => {
                let sent = self.on_session(move |session| async move {
                    match synced::create(&session, &name, &[], replace).await {
                        Ok(synced::Created::NameTaken) => Event::SyncNameTaken { list },
                        Ok(synced::Created::Made(uri)) => Event::SyncCreated {
                            list,
                            result: Ok(uri),
                        },
                        Err(e) => Event::SyncCreated {
                            list,
                            result: Err(format!("{e:#}")),
                        },
                    }
                });
                if !sent {
                    let result = Err("Sign in to sync a playlist.".to_string());
                    (self.emit)(Event::SyncCreated { list, result });
                }
            }
            Command::SyncFolder => {
                self.on_session(move |session| async move {
                    let result = synced::folder(&session).await.map_err(|e| format!("{e:#}"));
                    Event::SyncFolder(result)
                });
            }
            _ => {}
        }
    }
}
