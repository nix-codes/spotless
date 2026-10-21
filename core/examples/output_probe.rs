//! Check: does choosing an output move Spotless's stream there, both when
//! the choice is made first and the stream opens after, and when the stream
//! is already open? Plays silence only.
//!
//!   cargo run -p spotless-core --example output_probe -- [output name]
//!
//! Without a name it moves to the first output that isn't the default, and
//! back to the default at the end.

use std::process::Command;
use std::time::Duration;

use cpal::traits::HostTrait;
use spotless_core::playback::outputs;

fn main() {
    let Some(listed) = outputs::list() else {
        println!("outputs can't be listed here");
        return;
    };
    for output in &listed {
        println!("output: {} ({})", output.description, output.name);
    }
    let default = pactl(&["get-default-sink"]);
    let target = std::env::args()
        .nth(1)
        .or_else(|| {
            listed
                .iter()
                .map(|o| o.name.clone())
                .find(|n| *n != default)
        })
        .expect("a second output to move to");

    // Chosen first: the stream goes there as it opens.
    outputs::restore(Some(target.clone()));
    let device = cpal::default_host().default_output_device().unwrap();
    let stream = rodio::OutputStreamBuilder::from_device(device)
        .unwrap()
        .open_stream()
        .unwrap();
    let sink = rodio::Sink::connect_new(stream.mixer());
    sink.append(rodio::source::Zero::new(2, 44100));
    outputs::stream_opened();
    std::thread::sleep(Duration::from_secs(2));
    report("chosen before opening", &target);

    // Chosen while open: back to the default.
    outputs::choose(None);
    std::thread::sleep(Duration::from_secs(1));
    report("back to the default while open", &default);
}

fn report(what: &str, expected: &str) {
    let node = format!(
        "alsa_playback.{}",
        std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
    );
    let inputs: serde_json::Value =
        serde_json::from_str(&pactl(&["-f", "json", "list", "sink-inputs"])).unwrap();
    let sinks: serde_json::Value =
        serde_json::from_str(&pactl(&["-f", "json", "list", "sinks"])).unwrap();
    let on = inputs
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["properties"]["node.name"].as_str() == Some(&node))
        .and_then(|i| i["sink"].as_u64())
        .and_then(|index| {
            sinks
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["index"].as_u64() == Some(index))
        })
        .and_then(|s| s["name"].as_str())
        .unwrap_or("(stream not found)")
        .to_string();
    let verdict = if on == expected { "OK" } else { "WRONG" };
    println!("{verdict}: {what}: on {on}");
}

fn pactl(args: &[&str]) -> String {
    let output = Command::new("pactl").args(args).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
