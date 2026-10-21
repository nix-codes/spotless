//! Opening the approval pages: Spotify's sign-in, search's and Last.fm's.
//! The desktop opens them in the system's browser; an app the core can't
//! open a browser for (Android) is handed the page and opens it itself.

use std::sync::Arc;

pub type Browser = Arc<dyn Fn(&str) + Send + Sync>;

/// The system's browser.
pub fn system() -> Browser {
    Arc::new(|url| {
        if let Err(error) = open::that_detached(url) {
            log::error!("unable to open the browser: {error}");
        }
    })
}
