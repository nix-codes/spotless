//! Choosing the audio output.
//!
//! On Linux the output is chosen through the sound server (PipeWire or
//! PulseAudio, with `pactl`): ALSA lists cards unreliably while PipeWire
//! holds them, and a card opened directly bypasses the system's mixer.
//! Spotless keeps playing to the default device; its stream is then moved
//! to the chosen output, which the sound server also remembers for the
//! streams that follow.
//!
//! On Windows cpal's own device list is reliable, so the audio output
//! opens the chosen device by name ([`device`]), and opens it again when
//! the choice changes. Elsewhere nothing is listed yet, and the system's
//! sound settings decide.

use std::sync::Mutex;

/// An output the sound server offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The sound server's name for it, which the choice is saved as.
    pub name: String,
    /// What the system's sound settings call it.
    pub description: String,
}

/// The chosen output; None follows the system's choice.
static CHOICE: Mutex<Option<String>> = Mutex::new(None);

fn choice() -> Option<String> {
    CHOICE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The choice saved in the settings, at start: applied when a stream opens.
pub fn restore(output: Option<String>) {
    *CHOICE.lock().unwrap_or_else(|p| p.into_inner()) = output;
}

/// Chooses an output, moving a stream that is already open. None goes
/// back to the system's default output.
pub fn choose(output: Option<String>) {
    restore(output.clone());
    std::thread::spawn(move || imp::move_stream(output.as_deref().unwrap_or(imp::DEFAULT), 1));
}

/// Called when the audio output opens a stream: it goes to the chosen
/// output. The stream takes a moment to appear, so this keeps looking a
/// little while, on its own thread.
pub fn stream_opened() {
    if let Some(output) = choice() {
        std::thread::spawn(move || imp::move_stream(&output, 20));
    }
}

/// The outputs to choose from. None when they can't be listed here.
pub fn list() -> Option<Vec<Output>> {
    imp::list()
}

/// The device the audio output opens by name: the chosen one where the
/// stream isn't moved for it (Windows), else None for the default device.
pub fn device() -> Option<String> {
    if imp::OPENS_BY_NAME { choice() } else { None }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::process::Command;
    use std::time::Duration;

    use serde_json::Value;

    use super::Output;

    /// `pactl`'s name for whichever output is the default.
    pub const DEFAULT: &str = "@DEFAULT_SINK@";

    /// The stream opens on the default device and is moved.
    pub const OPENS_BY_NAME: bool = false;

    pub fn list() -> Option<Vec<Output>> {
        let sinks = pactl_json(&["list", "sinks"])?;
        Some(
            sinks
                .as_array()?
                .iter()
                .filter_map(|sink| {
                    Some(Output {
                        name: sink["name"].as_str()?.to_string(),
                        description: sink["description"].as_str()?.to_string(),
                    })
                })
                .collect(),
        )
    }

    /// Moves Spotless's playback stream to `output`, trying `attempts`
    /// times a tenth of a second apart while the stream isn't there yet.
    pub fn move_stream(output: &str, attempts: u32) {
        let node = stream_node_name();
        for attempt in 0..attempts {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(100));
            }
            let Some(inputs) = pactl_json(&["list", "sink-inputs"]) else {
                return;
            };
            let ours =
                inputs.as_array().into_iter().flatten().filter(|input| {
                    input["properties"]["node.name"].as_str() == Some(node.as_str())
                });
            let mut found = false;
            for input in ours {
                found = true;
                let Some(index) = input["index"].as_u64() else {
                    continue;
                };
                let moved = Command::new("pactl")
                    .args(["move-sink-input", &index.to_string(), output])
                    .status();
                if !moved.is_ok_and(|status| status.success()) {
                    log::warn!("unable to move the audio stream to {output}");
                }
            }
            if found {
                return;
            }
        }
    }

    /// PipeWire's ALSA plugin names a program's stream after it.
    fn stream_node_name() -> String {
        let program = std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "spotless".into());
        format!("alsa_playback.{program}")
    }

    fn pactl_json(args: &[&str]) -> Option<Value> {
        let output = Command::new("pactl")
            .args(["-f", "json"])
            .args(args)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        serde_json::from_slice(&output.stdout).ok()
    }
}

#[cfg(windows)]
mod imp {
    use cpal::traits::{DeviceTrait, HostTrait};

    use super::Output;

    pub const DEFAULT: &str = "";

    /// The audio output opens the chosen device itself.
    pub const OPENS_BY_NAME: bool = true;

    /// WASAPI's outputs, by the names the system's sound settings show.
    pub fn list() -> Option<Vec<Output>> {
        let devices = cpal::default_host().output_devices().ok()?;
        let mut outputs: Vec<Output> = Vec::new();
        for name in devices.filter_map(|device| device.name().ok()) {
            if !outputs.iter().any(|output| output.name == name) {
                outputs.push(Output {
                    description: name.clone(),
                    name,
                });
            }
        }
        Some(outputs)
    }

    /// Nothing to move: the audio output notices the new choice.
    pub fn move_stream(_output: &str, _attempts: u32) {}
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::Output;

    pub const DEFAULT: &str = "";

    pub const OPENS_BY_NAME: bool = false;

    pub fn list() -> Option<Vec<Output>> {
        None
    }

    pub fn move_stream(_output: &str, _attempts: u32) {}
}
