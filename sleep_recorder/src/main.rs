use std::env;
use sleep_recorder::sleep_tracker;


#[tokio::main]
async fn main() {
    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global tracing subscriber.");

    let data_path = env::var("SLEEP_DATA_DIR").expect("SLEEP_DATA_DIR not set");

    sleep_tracker(&data_path)
        .await
        .expect("Failed to start sleep tracker");
}