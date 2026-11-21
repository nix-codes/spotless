//! Check: does Spotless's own audio output follow the chosen output? It
//! lists the outputs, plays silence through the sink on the default one,
//! then chooses another while playing, then the default again. Where
//! outputs are chosen by device (Windows), the sink reopens on each new
//! choice; the log says which device it opened.
//!
//!   RUST_LOG=info cargo run -p spotless-core --features probes --example sink_probe -- [output name]
//!
//! Without a name it chooses the first output listed.

use std::time::{Duration, Instant};

use spotless_core::playback::audio_sink::AudioSink;
use spotless_core::playback::sink::RodioSink;
use spotless_core::playback::{AudioLink, outputs};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let Some(listed) = outputs::list() else {
        println!("outputs can't be listed here");
        return;
    };
    for output in &listed {
        println!("output: {} ({})", output.description, output.name);
    }
    let target = std::env::args()
        .nth(1)
        .or_else(|| listed.first().map(|o| o.name.clone()))
        .expect("an output to choose");

    let link = std::sync::Arc::<AudioLink>::default();
    let mut sink = RodioSink::new(
        None,
        Box::new(|message| println!("error: {message}")),
        std::sync::Arc::clone(&link),
    );
    sink.start().expect("the default output opens");
    play_silence(&mut sink, Duration::from_secs(2));

    println!("choosing {target}");
    outputs::choose(&link, Some(target));
    play_silence(&mut sink, Duration::from_secs(2));

    println!("back to the default");
    outputs::choose(&link, None);
    play_silence(&mut sink, Duration::from_secs(2));
    sink.stop();
    println!("done");
}

/// About 20 ms of stereo silence at a time, as the player writes it.
fn play_silence(sink: &mut RodioSink, length: Duration) {
    let began = Instant::now();
    while began.elapsed() < length {
        if let Err(error) = sink.write(vec![0.0; 2 * 882], 1.0) {
            println!("write failed: {error}");
            return;
        }
    }
}
