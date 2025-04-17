use ens160_aq::Ens160;
use tracing::{info, warn};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use std::{error::Error, time::{Duration, SystemTime, UNIX_EPOCH}};

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
    pub fn measure(&mut self) -> Option<bme280::Measurements<linux_embedded_hal::I2CError>> {
        let mut delay = Delay;
        self.bme280
            .measure(&mut delay)
            .map_err(|e| warn!("BME280 measurement error: {e}"))
            .ok()
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
    pub fn measure(&mut self, timestamp: &str) -> Option<String> {
        let frame = self.camera.capture().map_err(|e| {
            warn!("Camera capture error: {:?}", e);
        }).ok()?;
    
        let image_path = format!("{0}image_{timestamp}.jpg", self.image_directory);
        std::fs::write(&image_path, &*frame).map_err(|e| {
            warn!("Failed to save image: {:?}", e);
        }).ok()?;
    
        info!("Image saved to {}", image_path);
        Some(image_path)
    }
}

pub struct ENS160Wrapper {
    ens160: Ens160<I2cdev, Delay>,
}
impl ENS160Wrapper {
    pub fn new(cal_temp: f32, cal_humidity: f32) -> Result<Self, String> {
        let i2c_bus = I2cdev::new("/dev/i2c-1").map_err(|e| format!("I2C Initialization error: {:?}", e))?;
        let delay = Delay;
        let mut ens160 = Ens160::new_secondary_address(i2c_bus, delay);
        ens160.initialize().map_err(|e| format!("ENS160 Initialization error: {:?}", e))?;
        ens160.set_temp_rh_comp(cal_temp, (cal_humidity * 100.) as u16).map_err(|e| format!("ENS160 Initialization error: {:?}", e))?;
        std::thread::sleep(Duration::from_millis(500));  // wait for the sensor to stabilize
        Ok(Self { ens160 })
    }
    pub fn measure(&mut self) -> Option<ens160_aq::data::Measurements> {
        let status = self.ens160.get_status().map_err(|e| {
            warn!("ENS160 status error: {:?}", e);
        }).ok()?;

        if status.new_data_ready() {  // read all measurements
            return self.ens160.get_measurements().map_err(|e| {
                warn!("ENS160 measurement error: {:?}", e);
            }).ok()
        }
        
        warn!("No new data ready from ENS160.");
        None
    }
}

pub struct SensorReader {
    bme280: BME280Wrapper,
    ens160: ENS160Wrapper,
    camera: CameraWrapper,
}

impl SensorReader {
    #[tracing::instrument]
    pub fn new(data_path: &str) -> Result<Self, Box<dyn Error>> {
        let mut bme280 = BME280Wrapper::new()?;
        info!("BME280 initialized successfully.");

        let bme280_measurements = bme280.measure().ok_or("Failed to read BME280 measurements.")?;
        let ens160 = ENS160Wrapper::new(bme280_measurements.temperature, bme280_measurements.humidity)?;
        info!("ENS160 initialized successfully with cal temp of {}°C and {} RH.", bme280_measurements.temperature, bme280_measurements.humidity);

        let camera = CameraWrapper::new(data_path.to_string() + "/images/" )?;            
        info!("Camera initialized successfully.");
        
        Ok(Self { bme280, ens160, camera })
    }

    #[tracing::instrument(skip(self))]
    pub fn measure(&mut self) -> Result<SleepData, Box<dyn Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut builder = SleepData::builder(timestamp);

        if let Some(bme280_measurements) = self.bme280.measure() {
            builder = builder.with_bme280(bme280_measurements);
        } 
        if let Some(ens160_measurements) = self.ens160.measure() {
            builder = builder.with_ens160(ens160_measurements);
        } 
        if let Some(image_path) = self.camera.measure(&timestamp.to_string()) {
            builder = builder.with_image_path(image_path);
        }

        Ok(builder.build())
    }
}
