use std::str::FromStr;
use hdf5::{File, types::VarLenUnicode};

use tracing::{debug, error, info, span, warn, Level};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use std::time::{SystemTime, UNIX_EPOCH};

use std::error::Error;
use std::result::Result;

#[derive(Debug)]
pub struct SleepData {
    timestamp: u64,
    temperature: f32,
    pressure: f32,
    humidity: f32,
    image_path: String,
}

#[derive(Debug)]
pub struct SleepDataLogger {
    buffer: Vec<SleepData>,
    flush_every: usize,
    file: File,
    group_name: String,
}

impl SleepDataLogger {
    pub fn new(data_path: &str, file_name: &str, group_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::create(data_path.to_string() + "/" + file_name)?;
        let group = file.create_group(group_name)?;

        // Create datasets for each field in the group
        // let builder = group.new_dataset_builder()
        //     .shape(hdf5::SimpleExtents::resizable(&[0]))
        //     .chunk((1024,))
        //     .deflate(6);

        group.new_dataset::<u64>().shape(hdf5::SimpleExtents::resizable(&[0])).chunk(1024).deflate(6).create("timestamp")?;
        group.new_dataset::<f32>().shape(hdf5::SimpleExtents::resizable(&[0])).chunk(1024).deflate(6).create("temperature")?;
        group.new_dataset::<f32>().shape(hdf5::SimpleExtents::resizable(&[0])).chunk(1024).deflate(6).create("pressure")?;
        group.new_dataset::<f32>().shape(hdf5::SimpleExtents::resizable(&[0])).chunk(1024).deflate(6).create("humidity")?;
        group.new_dataset::<VarLenUnicode>().shape(hdf5::SimpleExtents::resizable(&[0])).chunk(1024).deflate(6).create("image_path")?;

        info!("HDF5 file ({file_name}) and group ({group_name}) created successfully at {data_path}.");

        Ok(Self {
            buffer: Vec::new(),
            flush_every: 5,
            file,
            group_name: group_name.to_string(),
        })
    }

    #[tracing::instrument(skip(self, sample))]
    pub fn append(&mut self, sample: SleepData) -> Result<(), Box<dyn Error>> {
        info!("Pushing sample to buffer: {:?}", &sample);
        self.buffer.push(sample);
        if self.buffer.len() >= self.flush_every {
            info!("Flushing data to HDF5 file...");
            self.flush()?;
        }
        Ok(())
    }

    #[tracing::instrument]
    pub fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        let buffer = std::mem::take(&mut self.buffer);
        let file = self.file.clone();
        let group_name = self.group_name.clone();

        if buffer.is_empty() {
            return Ok(());
        }

        let group = file.group(&group_name)?;

        // Append data to each dataset
        let timestamps: Vec<u64> = buffer.iter().map(|d| d.timestamp).collect();
        let temperatures: Vec<f32> = buffer.iter().map(|d| d.temperature).collect();
        let pressures: Vec<f32> = buffer.iter().map(|d| d.pressure).collect();
        let humidities: Vec<f32> = buffer.iter().map(|d| d.humidity).collect();
        let image_paths: Vec<VarLenUnicode> = buffer.iter()
            .map(|d| VarLenUnicode::from_str(&d.image_path))
            .collect::<Result<_, _>>()?;

        let current_size = group.dataset("timestamp")?.shape()[0];
        let new_size = current_size + buffer.len();
        group.dataset("timestamp")?.resize(new_size)?;
        group.dataset("temperature")?.resize(new_size)?;
        group.dataset("pressure")?.resize(new_size)?;
        group.dataset("humidity")?.resize(new_size)?;
        group.dataset("image_path")?.resize(new_size)?;

        group.dataset("timestamp")?.write_slice(&timestamps, (current_size..new_size,))?;
        group.dataset("temperature")?.write_slice(&temperatures, (current_size..new_size,))?;
        group.dataset("pressure")?.write_slice(&pressures, (current_size..new_size,))?;
        group.dataset("humidity")?.write_slice(&humidities, (current_size..new_size,))?;
        group.dataset("image_path")?.write_slice(&image_paths, (current_size..new_size,))?;

        info!("Successfully flushed, size increased from {current_size} to {new_size}");
        Ok(())
    }
}

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
            interval: (1, 30),            // ~1 FPS (will capture one frame per call)
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

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();        ;
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
