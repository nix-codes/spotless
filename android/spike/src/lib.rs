//! Spotless on Android, the feasibility spike: signs in, plays one song and
//! keeps playing in the background, with the desktop's own session, engine
//! and audio output (included below as they are, not copied).
//!
//! Java asks through the `Native` class and polls [`status`] and the
//! sign-in URL; nothing here calls back into Java.

#![allow(dead_code, unused_imports)]

// The desktop core, unchanged.
#[path = "../../../src/model/mod.rs"]
mod model;
#[path = "../../../src/paths.rs"]
mod paths;
#[path = "../../../src/store.rs"]
mod store;
#[path = "../../../src/playback"]
mod playback {
    pub mod engine;
    pub mod outputs;
    pub mod sink;
}
#[path = "../../../src/spotify"]
mod spotify {
    pub mod session;
}

use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::jstring;
use librespot_core::SpotifyUri;
use librespot_core::session::Session;

use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::spotify::session;

/// Where Android keeps the system's certificate authorities: the updatable
/// set first (Android 14 on), then the one in the system image.
const CA_DIRS: [&str; 2] = [
    "/apex/com.android.conscrypt/cacerts",
    "/system/etc/security/cacerts",
];

struct Spike {
    runtime: tokio::runtime::Runtime,
    paths: Paths,
    session: Mutex<Option<Session>>,
    engine: Mutex<Option<Engine>>,
    /// One line for the screen.
    status: Mutex<String>,
    /// The sign-in page, until Java opens it.
    url: Mutex<Option<String>>,
}

static SPIKE: OnceLock<Spike> = OnceLock::new();

fn spike() -> &'static Spike {
    SPIKE.get().expect("Native.init not called")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

fn say(line: impl Into<String>) {
    let line = line.into();
    log::info!("status: {line}");
    *lock(&spike().status) = line;
}

/// Sets up logging, certificates and cpal's context, then reconnects with
/// the saved credential if there is one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_init(
    mut env: JNIEnv,
    _class: JClass,
    context: JObject,
    config: JString,
    cache: JString,
) {
    if SPIKE.get().is_some() {
        return;
    }
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("spotless"),
    );
    // rustls-native-certs finds no certificates on Android by itself
    // (openssl-probe only knows Termux's folders), but reads this.
    if let Some(dir) = CA_DIRS.iter().find(|d| std::path::Path::new(d).is_dir()) {
        // SAFETY: no other thread of ours runs yet.
        unsafe { std::env::set_var("SSL_CERT_DIR", dir) };
        log::info!("certificates from {dir}");
    }
    capture_stdout();
    // cpal's AAudio backend asks Java for the devices.
    let vm = env.get_java_vm().expect("no Java VM");
    let context = env.new_global_ref(context).expect("no global ref");
    // SAFETY: both stay valid for the life of the process (the reference
    // is leaked below).
    unsafe {
        ndk_context::initialize_android_context(
            vm.get_java_vm_pointer().cast(),
            context.as_obj().as_raw().cast(),
        );
    }
    std::mem::forget(context);

    let config: String = env.get_string(&config).expect("config path").into();
    let cache: String = env.get_string(&cache).expect("cache path").into();
    let paths = Paths {
        config: PathBuf::from(config).join("spotless"),
        cache: PathBuf::from(cache).join("spotless"),
    };
    for dir in [&paths.config, &paths.cache] {
        let _ = std::fs::create_dir_all(dir);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("no tokio runtime");
    let _ = SPIKE.set(Spike {
        runtime,
        paths,
        session: Mutex::default(),
        engine: Mutex::default(),
        status: Mutex::new("Starting".into()),
        url: Mutex::default(),
    });
    spike().runtime.spawn(async {
        say("Connecting");
        match session::restore(&spike().paths).await {
            Ok(Some(session)) => ready(session),
            Ok(None) => say("Signed out: press Sign in"),
            Err(error) => say(format!("Reconnecting failed: {error:#}")),
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_signIn(_env: JNIEnv, _class: JClass) {
    spike().runtime.spawn(async {
        say("Waiting for the browser");
        match session::sign_in(&spike().paths).await {
            Ok(session) => ready(session),
            Err(error) => say(format!("Sign-in failed: {error:#}")),
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_play(
    mut env: JNIEnv,
    _class: JClass,
    link: JString,
) {
    let link: String = env.get_string(&link).map(Into::into).unwrap_or_default();
    let uri = match SpotifyUri::from_uri(link.trim()) {
        Ok(uri) => uri,
        Err(error) => return say(format!("Not a track URI: {error}")),
    };
    spike().runtime.spawn(async move {
        // Android cuts an idle app's network (screen off, app in the
        // background), and Spotify then drops the session: reconnect with
        // the saved credential first.
        let dropped = lock(&spike().session)
            .as_ref()
            .is_some_and(|s| s.is_invalid());
        if dropped {
            say("Reconnecting");
            lock(&spike().engine).take();
            match session::restore(&spike().paths).await {
                Ok(Some(session)) => ready(session),
                Ok(None) => return say("Signed out: press Sign in"),
                Err(error) => return say(format!("Reconnecting failed: {error:#}")),
            }
        }
        match lock(&spike().engine).as_ref() {
            Some(engine) => engine.load(uri, 0),
            None => say("Not signed in"),
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_pause(_env: JNIEnv, _class: JClass) {
    if let Some(engine) = lock(&spike().engine).as_ref() {
        engine.pause();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_resume(_env: JNIEnv, _class: JClass) {
    if let Some(engine) = lock(&spike().engine).as_ref() {
        engine.play();
    }
}

/// The line for the screen.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_status(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let status = SPIKE
        .get()
        .map(|s| lock(&s.status).clone())
        .unwrap_or_default();
    env.new_string(status)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// The sign-in page to open, once; null when there is none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_spike_Native_takeUrl(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    match SPIKE.get().and_then(|s| lock(&s.url).take()) {
        Some(url) => env
            .new_string(url)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// Makes the engine on the new session.
fn ready(session: Session) {
    let user = session.username();
    let notify = Arc::new(|event: EngineEvent| match event {
        EngineEvent::Snapshot(snapshot) => say(describe(&snapshot)),
        EngineEvent::OutputError(message) => say(message),
        EngineEvent::Unavailable => say("That song can't be played"),
        EngineEvent::Ended => say("Ended"),
        EngineEvent::Followed(_) => {}
    });
    match Engine::new(&session, u16::MAX / 2, false, notify) {
        Ok(engine) => {
            *lock(&spike().engine) = Some(engine);
            *lock(&spike().session) = Some(session);
            say(format!("Signed in as {user}: press Play"));
        }
        Err(error) => say(format!("No player: {error:#}")),
    }
}

fn describe(snapshot: &PlayerSnapshot) -> String {
    let position = snapshot.position_now() / 1000;
    let track = match &snapshot.track {
        Some(t) => format!("{} – {}", t.artists, t.title),
        None => "nothing".into(),
    };
    format!(
        "{:?}: {track} at {}:{:02}",
        snapshot.status,
        position / 60,
        position % 60
    )
}

/// librespot-oauth only prints the sign-in page ("Browse to: …"), and on
/// Android stdout goes nowhere. This sends it to the log instead and keeps
/// the page for Java to open.
fn capture_stdout() {
    let mut fds = [0; 2];
    // SAFETY: plain pipe and dup2 on descriptors we own.
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 || libc::dup2(fds[1], libc::STDOUT_FILENO) < 0 {
            log::warn!("unable to capture stdout");
            return;
        }
    }
    // SAFETY: the read end is ours alone from here.
    let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            log::info!("stdout: {line}");
            if let Some(url) = line.strip_prefix("Browse to: ")
                && let Some(spike) = SPIKE.get()
            {
                *lock(&spike.url) = Some(url.to_string());
            }
        }
    });
}
