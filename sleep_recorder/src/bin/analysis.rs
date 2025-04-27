use std::env;

use tracing::info;
use sleep_recorder::audio_analysis::{analyze_audio_entries, block_rms, decode_mp3};


#[tokio::main]
async fn main() {
    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global tracing subscriber.");

    let data_path = env::var("SLEEP_DATA_DIR").expect("SLEEP_DATA_DIR not set");

    
    info!("Starting sleep_recorder analysis");
    // let audio_path = "/mnt/usb/sleep_data/audio/audio_1745613034.mp3";
    // let samples = decode_mp3(audio_path).expect("Failed to decode mp3");
    // info!("Sample length: {}", samples.len());
    // let window_size_s = 5;
    // let block_rms = block_rms(samples, window_size_s);
    // info!("Block RMS: {:?}", block_rms.len());
    analyze_audio_entries(&data_path, "sleep_data.h5", "2025-04-17_23-01-13").expect("Failed to analyze audio entries");

}