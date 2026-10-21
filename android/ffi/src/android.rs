//! What Android needs before the core starts, which UniFFI can't carry: the
//! app's `Context`. Kotlin calls `dev.spotless.core.Native.init(context)`
//! once, before it makes a `Spotless`.

use jni::EnvUnowned;
use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject};

/// Sets up the log (`adb logcat -s spotless`), Android's certificate
/// verifier and cpal's context.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_core_Native_init<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    context: JObject<'local>,
) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("spotless"),
        );
        // Android drops stderr: a panic would otherwise say nothing.
        std::panic::set_hook(Box::new(|info| {
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("unnamed");
            log::error!("thread {name} panicked: {info}");
        }));
        env.with_env(|env| -> jni::errors::Result<()> {
            // cpal's AAudio backend asks Java for the devices.
            let vm = env.get_java_vm()?;
            let global = env.new_global_ref(&context)?;
            // SAFETY: both stay valid for the life of the process (the
            // reference is leaked below).
            unsafe {
                ndk_context::initialize_android_context(
                    vm.get_raw().cast(),
                    global.as_raw().cast(),
                );
            }
            std::mem::forget(global);
            // Every TLS connection is checked by Android's own verifier
            // (rustls-platform-verifier), whose Kotlin part the app ships.
            rustls_platform_verifier::android::init_with_env(env, context)
        })
        .resolve::<ThrowRuntimeExAndDefault>();
    });
}
