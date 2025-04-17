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
    co2eq_ppm: u16,
    tvoc_ppb: u16,
    air_quality_index: u16,
    image_path: String,
}
impl SleepData {
    pub fn builder(timestamp: u64) -> SleepDataBuilder {
        SleepDataBuilder::new(timestamp)
    }
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

    pub fn build(self) -> SleepData {
        SleepData {
            timestamp: self.timestamp,
            temperature: self.temperature.unwrap_or(f32::NAN),
            pressure: self.pressure.unwrap_or(f32::NAN),
            humidity: self.humidity.unwrap_or(f32::NAN),
            co2eq_ppm: self.co2eq_ppm.unwrap_or_default(),
            tvoc_ppb: self.tvoc_ppb.unwrap_or_default(),
            air_quality_index: self.air_quality_index.unwrap_or_default(),
            image_path: self.image_path.unwrap_or_default(),
        }
    }
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
    let mut data_logger = SleepDataLogger::new(data_path, "sleep_data.h5")?;

    let mut sensor_reader = SensorReader::new(data_path)?;
    info!("DB & sensor reader initialized.");

    tokio::select! {
        _ = collect_sensor_data(&mut data_logger, &mut sensor_reader) => {
            // This branch will run until the sensor polling is interrupted
            info!("Sensor polling completed.");
        },
        _ = clonced_cancel.cancelled() => {
            info!("Recevied shutdown signal.");
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