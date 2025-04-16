use tracing::{info, warn};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use std::{error::Error, time::{SystemTime, UNIX_EPOCH}};

use super::SleepData;

pub struct BME280Wrapper {
    bme280: BME280<I2cdev>,
}
impl BME280Wrapper {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let i2c_bus = I2cdev::new("/dev/i2c-1")?;
        let mut delay = Delay;
        let mut bme280 = BME280::new_primary(i2c_bus);
        bme280.init(&mut delay)?;
        Ok(Self { bme280 })
    }
    pub fn measure(&mut self) -> Result<bme280::Measurements<linux_embedded_hal::I2CError>, Box<dyn Error>> {
        let mut delay = Delay;
        let measurements = self.bme280.measure(&mut delay)?;
        Ok (measurements)
    }
}

pub struct CameraWrapper {
    camera: Camera,
    image_directory: String
}
impl CameraWrapper {
    pub fn new(image_directory: String) -> Result<Self, Box<dyn Error>> {
        let mut camera = Camera::new("/dev/video0")?;
        camera.start(&Config {
            interval: (1, 30),          
            resolution: (1280, 720),
            format: b"MJPG",             // MJPEG is widely supported
            ..Default::default()
        })?;
        Ok(Self { camera, image_directory })
    }
    pub fn measure(&mut self, timestamp: &str) -> Result<String, Box<dyn Error>> {
        let frame = self.camera.capture()?;
        let image_path = format!("{0}image_{timestamp}.jpg", self.image_directory);
        std::fs::write(&image_path, &*frame)?;
        Ok(image_path)
    }
}

pub struct SensorReader {
    bme280: BME280Wrapper,
    camera: CameraWrapper,
}

impl SensorReader {
    #[tracing::instrument]
    pub fn new(data_path: &str) -> Result<Self, Box<dyn Error>> {
        let bme280 = BME280Wrapper::new()?;
        info!("BME280 initialized successfully.");

        let camera = CameraWrapper::new(data_path.to_string() + "/images/" )?;            
        info!("Camera initialized successfully.");
        
        Ok(Self { bme280, camera })
    }

    #[tracing::instrument(skip(self))]
    pub fn measure(&mut self) -> Result<SleepData, Box<dyn Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut builder = SleepData::builder(timestamp);

        builder = builder.with_bme280(self.bme280.measure()?);
        builder = builder.with_image_path(self.camera.measure(&timestamp.to_string())?);

        Ok(builder.build())
    }
}
