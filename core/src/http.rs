//! The HTTP client for Spotify's Web API, Last.fm and the covers.

/// A client checking certificates the system's way: against its roots on
/// the desktop, through Android's own verifier on Android, where the
/// roots alone aren't found (the app initializes rustls-platform-verifier
/// first).
pub fn client() -> reqwest::Client {
    #[cfg(target_os = "android")]
    {
        use rustls_platform_verifier::ConfigVerifierExt;
        let built = rustls::ClientConfig::with_platform_verifier()
            .map_err(|e| e.to_string())
            .and_then(|tls| {
                reqwest::Client::builder()
                    .use_preconfigured_tls(tls)
                    .build()
                    .map_err(|e| e.to_string())
            });
        match built {
            Ok(client) => return client,
            Err(error) => log::error!("unable to use Android's certificate verifier: {error}"),
        }
    }
    reqwest::Client::new()
}
