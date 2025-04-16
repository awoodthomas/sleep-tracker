use std::error::Error;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

use data::SleepDataLogger;
use sensor::SensorReader;

pub mod sensor;
pub mod data;

#[derive(Debug)]
pub struct SleepData {
    timestamp: u64,
    temperature: f32,
    pressure: f32,
    humidity: f32,
    image_path: String,
}

pub async fn sleep_tracker(data_path: &str) -> Result<(), Box<dyn Error>> {
    let cancel = CancellationToken::new();
    let clonced_cancel = cancel.clone();

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        info!("Shutdown signal received.");
        cancel.cancel();
    });

    // Open file & initialize buffer
    let mut data_logger = SleepDataLogger::new(&data_path, "sleep_data.h5")?;

    let mut sensor_reader = SensorReader::new(&data_path)?;
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

    info!("Logger stopped.");
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