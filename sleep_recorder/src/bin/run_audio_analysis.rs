use std::env;

use tracing::info;
use sleep_recorder::audio_analysis::analyze_audio_entries;


#[tokio::main]
async fn main() {
    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global tracing subscriber.");

    let data_path = env::var("SLEEP_DATA_DIR").expect("SLEEP_DATA_DIR not set");

    
    info!("Starting sleep_recorder analysis");
    analyze_audio_entries(&data_path, "sleep_data.h5", "2025-04-30_22-47-31").expect("Failed to analyze audio entries");
}
