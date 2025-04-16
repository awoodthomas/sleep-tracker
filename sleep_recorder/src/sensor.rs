use tracing::{info, warn};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use std::{error::Error, time::{SystemTime, UNIX_EPOCH}};

use super::SleepData;

pub struct SensorReader {
    bme280: Option<BME280<I2cdev>>,
    camera: Option<Camera>,
    image_directory: String,
}

impl SensorReader {

    #[tracing::instrument]
    pub fn new(data_path: &str) -> Result<Self, Box<dyn Error>> {
        let i2c_bus = I2cdev::new("/dev/i2c-1")?;
        let mut delay = Delay;
        // initialize the BME280 using the primary I2C address 0x76
        let mut bme280 = BME280::new_primary(i2c_bus);
        bme280.init(&mut delay).unwrap();

        info!("BME280 initialized successfully.");

        let mut camera = Camera::new("/dev/video0")?;
        camera.start(&Config {
            interval: (1, 30),          
            resolution: (1280, 720),
            format: b"MJPG",             // MJPEG is widely supported
            ..Default::default()
        })?;
        info!("Camera initialized successfully.");
        
        Ok(Self { bme280: Some(bme280) , camera: Some(camera) , image_directory: data_path.to_string() + "/images/" })
    }

    #[tracing::instrument(skip(self))]
    pub fn measure(&mut self) -> Result<SleepData, Box<dyn Error>> {
        if self.bme280.is_none() {
            return Err("Sensor not initialized".into());
        }
        let mut delay = Delay;
        let measurements = self.bme280.as_mut().unwrap().measure(&mut delay).unwrap();

        info!("Sensor measurements: {:?}", &measurements);

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let temperature = measurements.temperature;
        let pressure = measurements.pressure; // Placeholder for CO2 data
        let humidity = measurements.humidity;

        let frame = self.camera.as_mut().unwrap().capture()?;
        let image_path = format!("{0}image_{timestamp}.jpg", self.image_directory);
        std::fs::write(&image_path, &*frame)?;

        info!("Captured image: {}", &image_path);

        Ok(SleepData {
            timestamp,
            temperature,
            pressure,
            humidity,
            image_path,
        })
    }
}
