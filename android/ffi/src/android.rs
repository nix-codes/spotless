//! What Android needs before the core starts, which UniFFI can't carry: the
//! app's `Context`. Kotlin calls `dev.spotless.core.Native.init(context)`
//! once, before it makes a `Spotless`.

use jni::JNIEnv;
use jni::objects::{JClass, JObject};

/// Where Android keeps the system's certificate authorities: the updatable
/// set first (Android 14 on), then the one in the system image.
const CA_DIRS: [&str; 2] = [
    "/apex/com.android.conscrypt/cacerts",
    "/system/etc/security/cacerts",
];

/// Sets up the log (`adb logcat -s spotless`), the certificates and cpal's
/// context.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_spotless_core_Native_init(
    env: JNIEnv,
    _class: JClass,
    context: JObject,
) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("spotless"),
        );
        // rustls-native-certs finds no certificates on Android by itself
        // (openssl-probe only knows Termux's folders), but reads this.
        if let Some(dir) = CA_DIRS.iter().find(|d| std::path::Path::new(d).is_dir()) {
            // SAFETY: called before the core starts any thread.
            unsafe { std::env::set_var("SSL_CERT_DIR", dir) };
            log::info!("certificates from {dir}");
        }
        // cpal's AAudio backend asks Java for the devices.
        let vm = env.get_java_vm().expect("no Java VM");
        let context = env.new_global_ref(context).expect("no global ref");
        // SAFETY: both stay valid for the life of the process (the
        // reference is leaked below).
        unsafe {
            ndk_context::initialize_android_context(
                vm.get_java_vm_pointer().cast(),
                context.as_obj().as_raw().cast(),
            );
        }
        std::mem::forget(context);
    });
}
