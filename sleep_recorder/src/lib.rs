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
// pub mod audio_analysis;

#[derive(Debug)]
pub struct SleepData {
    timestamp: u64,
    temperature: f32,
    pressure: f32,
    humidity: f32,
    co2eq_ppm: u16,
    tvoc_ppb: u16,
    air_quality_index: u16,
    thermistor_temp: f32,
    image_path: String,
}
impl SleepData {
    pub fn builder(timestamp: u64) -> SleepDataBuilder {
        SleepDataBuilder::new(timestamp)
    }
}

#[derive(Debug)]
pub struct AudioRecording {
    pub path: String,
    pub duration: Duration,
    pub start_time: u64,
}

#[derive(Default)]
pub struct SleepDataBuilder {
    timestamp: u64,
    temperature: Option<f32>,
    pressure: Option<f32>,
    humidity: Option<f32>,
    co2eq_ppm: Option<u16>,
    tvoc_ppb: Option<u16>,
    air_quality_index: Option<u16>,
    thermistor_temp: Option<f32>,
    image_path: Option<String>,
}

impl SleepDataBuilder {
    pub fn new(timestamp: u64) -> Self {
        Self {
            timestamp,
            ..Self::default()
        }
    }

    pub fn with_bme280(mut self, measurements: bme280::Measurements<linux_embedded_hal::I2CError>) -> Self {
        self.temperature = Some(measurements.temperature);
        self.pressure = Some(measurements.pressure);
        self.humidity = Some(measurements.humidity);
        self
    }

    pub fn with_ens160(mut self, measurements: ens160_aq::data::Measurements) -> Self {
        self.co2eq_ppm = Some(measurements.co2eq_ppm.value);
        self.tvoc_ppb = Some(measurements.tvoc_ppb);
        self.air_quality_index = Some(measurements.air_quality_index as u16);
        self
    }

    pub fn with_image_path(mut self, image_path: String) -> Self {
        self.image_path = Some(image_path);
        self
    }

    pub fn with_thermistor_temp(mut self, thermistor_temp: f32) -> Self {
        self.thermistor_temp = Some(thermistor_temp);
        self
    }

    pub fn build(self) -> SleepData {
        SleepData {
            timestamp: self.timestamp,
            temperature: self.temperature.unwrap_or(f32::NAN),
            pressure: self.pressure.unwrap_or(f32::NAN),
            humidity: self.humidity.unwrap_or(f32::NAN),
            co2eq_ppm: self.co2eq_ppm.unwrap_or_default(),
            tvoc_ppb: self.tvoc_ppb.unwrap_or_default(),
            air_quality_index: self.air_quality_index.unwrap_or_default(),
            thermistor_temp: self.thermistor_temp.unwrap_or(f32::NAN),
            image_path: self.image_path.unwrap_or_default(),
        }
    }
}

pub async fn sleep_tracker(data_path: &str) -> Result<(), Box<dyn Error>> {
    // 1) Setup
    let cancel = CancellationToken::new();
    let sensor_cancel = cancel.clone();
    let audio_cancel  = cancel.clone();

    let data_logger   = Arc::new(Mutex::new(
        SleepDataLogger::new(data_path, "sleep_data.h5")?));
    let sensor_reader = Arc::new(Mutex::new(
        SensorReader::new(data_path)?));
    let audio_recorder = Arc::new(
        AudioRecorder::new(
            format!("{}/audio", data_path),
            Duration::from_secs(60*30),
            "plughw:1,0".to_string(),
        ));

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
    let mut interval = tokio::time::interval(recorder.recording_time);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("audio_loop: shutdown");
                break;
            }
            _ = interval.tick() => {
                match recorder.async_audio_recording().await {
                    Ok(rec) => {
                        let path = rec.path.clone();
                        if let Ok(_) = data_logger.lock().await.add_audio_entry(rec)   {
                            info!("audio saved to {:?}", path);
                        }
                    }
                    Err(e) => warn!("audio error: {}", e),
                }
            }
        }
    }
}
