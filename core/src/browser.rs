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

/// The page an approval's redirect shows once it's done, as HTML. On
/// Android it links back to the app, which the browser may not leave for
/// by itself (the app takes `spotless://` links).
pub fn done_page(text: &str) -> String {
    let next = if cfg!(target_os = "android") {
        "<p><a href=\"spotless://back\" style=\"display:inline-block;padding:14px 22px;\
         background:#363646;color:#2bff62;text-decoration:none;font-weight:600\">\
         BACK TO SPOTLESS</a></p>"
    } else {
        "<p>You can close this tab.</p>"
    };
    format!(
        "<!doctype html><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>Spotless</title>\
         <body style=\"font:16px sans-serif;margin:3em\"><p>{text}</p>{next}"
    )
}

/// The body for librespot's OAuth redirect, which is sent without a type:
/// plain text on the desktop, as it always was; on Android, the page with
/// the way back, which the browser takes for HTML.
pub fn oauth_message(text: &str) -> String {
    if cfg!(target_os = "android") {
        done_page(text)
    } else {
        format!("{text} You can close this tab.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_pages_say_what_was_done() {
        let page = done_page("Spotless is connected to Last.fm.");
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<p>Spotless is connected to Last.fm.</p>"));
        assert_eq!(
            oauth_message("Spotless is signed in."),
            "Spotless is signed in. You can close this tab."
        );
    }
}
