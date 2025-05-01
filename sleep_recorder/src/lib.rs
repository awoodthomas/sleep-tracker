//! Module for a Raspberry Pi sleep recording device.
//! Users call the `sleep_tracker` function to start the application.

use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use data::SleepDataLogger;
use sensor::{AudioRecorder, SensorReader};
// use audio_analysis::decode_mp3;

pub mod sensor;
pub mod data;
pub mod audio_analysis;
pub mod image_analysis;

/// Starts the sleep tracker application. 
/// 
/// Creates a DataLogger, SensorReader, and AudioRecorder, and spawns two separate tasks
/// for reading sensor data and recording audio.
/// The tasks run concurrently and are cancelled when either the user interrupts the program.
/// Times out after 10 hours if the user does not interrupt.
/// 
/// # Arguments
/// 
/// * `data_path` - The path to the directory where data will be stored.
/// 
/// 
/// # Example
/// 
/// ```
/// use sleep_recorder::sleep_tracker;
/// 
/// #[tokio::main]
/// async fn main() {
///    let data_path = "/path/to/data";
///   if let Err(e) = sleep_tracker(data_path).await {
///       eprintln!("Error: {}", e);
///  }
/// }
/// ```
/// # Errors
/// 
/// If any of the initialization steps fail, an error is returned.
/// Individual failures of sensor or audio recording tasks are logged but do not cause the entire application to fail.
/// 
pub async fn sleep_tracker(data_path: &str) -> Result<(), Box<dyn Error>> {
    // 1) Setup
    let cancel = CancellationToken::new();
    let sensor_cancel = cancel.clone();
    let audio_cancel  = cancel.clone();

    let data_logger   = Arc::new(Mutex::new(
        SleepDataLogger::new(data_path, "sleep_data.h5")?));
    let sensor_reader = Arc::new(Mutex::new(
        SensorReader::new(data_path, &data_logger.lock().await.group_name)?));
    let audio_recorder = Arc::new(
        AudioRecorder::new(
            &format!("{}/{}/audio/", data_path, &data_logger.lock().await.group_name),
            Duration::from_secs(30*60),
            "plughw:1,0".to_string(),
        )?);

    // 2) Spawn the sensor‐polling task
    let mut sensor_handle = tokio::spawn(sensor_loop(sensor_cancel, data_logger.clone(), sensor_reader.clone()));
    let mut audio_handle  = tokio::spawn(audio_loop(audio_cancel, data_logger.clone(), audio_recorder.clone()));

    // 4) Top‐level select: Ctrl‑C, timeout, or task failures
    let timeout = tokio::time::sleep(Duration::from_secs(60 * 60 * 10)); // 10 h
    tokio::pin!(timeout);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl‑C received; cancelling...");
            cancel.cancel();
        }

        _ = &mut timeout => {
            info!("Timeout reached; cancelling...");
            cancel.cancel();
        }

        // If either background task panics or returns:
        res = &mut sensor_handle => {
            if let Err(e) = res {
                error!("Sensor task aborted: {:?}", e);
                cancel.cancel();
            }
        }
        res = &mut audio_handle => {
            if let Err(e) = res {
                error!("Audio task aborted: {:?}", e);
                cancel.cancel();
            }
        }
    }

    // 5) Wait for both loops to finish cleanly
    let _ = sensor_handle.await;
    let _ = audio_handle.await;

    info!("All loops exited; sleep_tracker done.");
    Ok(())
}

async fn sensor_loop(
    cancel: CancellationToken,
    data_logger: Arc<Mutex<SleepDataLogger>>,
    sensor_reader: Arc<Mutex<SensorReader>>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("sensor_loop: shutdown");
                break;
            }
            _ = interval.tick() => {
                let sample = match sensor_reader.lock().await.measure() {
                    Ok(s)  => s,
                    Err(e) => { warn!("sensor read error: {}", e); continue; }
                };
                if let Err(e) = data_logger.lock().await.append(sample) {
                    warn!("log append error: {}", e);
                }
            }
        }
    }
}

async fn audio_loop(
    cancel: CancellationToken,
    data_logger: Arc<Mutex<SleepDataLogger>>,
    recorder: Arc<AudioRecorder>,
) {
    while !cancel.is_cancelled() {
        // Start a cancellable recording
        match recorder.async_audio_recording().await {
            Ok(rec) => {
                let path = rec.path.clone();
                if data_logger.lock().await.add_audio_entry(rec).is_ok() {
                    info!("audio saved to {:?}", path);
                }
            }
            Err(e) => {
                if cancel.is_cancelled() {
                    info!("audio_loop: recording cancelled early: {e}");
                    break;
                } else {
                    warn!("audio error: {e}");
                }
            }
        }
    }

    info!("audio_loop: shutdown complete");
}
