use std::error::Error;
use std::time::Duration;
use std::env;
use sleep_recorder::{SleepDataLogger, SensorReader};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, span, warn, Level};



#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber)?;

    let data_path = env::var("SLEEP_DATA_DIR").expect("SLEEP_DATA_DIR not set");

    let cancel = CancellationToken::new();
    let clonced_cancel = cancel.clone();

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        println!("Shutdown signal received.");
        cancel.cancel();
    });

    // Open file & initialize buffer
    let mut data_logger = SleepDataLogger::new(&data_path, "sleep_data.h5", "2025-04-14_test").expect("Could not initialize the HDF5 dataset, cannot start sleep tracker.");

    let mut sensor_reader = SensorReader::new(&data_path).expect("Failed to initialize sensor reader.");
    info!("DB & sensor reader initialized.");

    tokio::select! {
        _ = collect_sensor_data(&mut data_logger, &mut sensor_reader) => {
            // This branch will run until the sensor polling is interrupted
            info!("Sensor polling completed.");
        },
        _ = clonced_cancel.cancelled() => {
            info!("Recevied shutdown signal. Flushing final data...");
            data_logger.flush().expect("Final flush failed."); // Final flush
        }
    }

    println!("Logger stopped.");
    Ok(())
}

async fn collect_sensor_data(data_logger: &mut SleepDataLogger, sensor_reader: &mut SensorReader) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        interval.tick().await;
        let sample = sensor_reader.measure().expect("Failed to read sensor data.");
        data_logger.append(sample).expect("Failed to append data to logger.");
    }
}