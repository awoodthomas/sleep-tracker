use ens160_aq::Ens160;
use mcp342x::{Channel, Gain, MCP342x, Resolution};
use tokio::process::Command;
use tracing::{info, warn};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use std::{error::Error, time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH}};

use crate::AudioRecording;

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

pub struct ThermistorWrapper {
    adc: MCP342x<I2cdev>,
}
impl ThermistorWrapper {
    const R_I: f32 = 3200.0; // Voltage divider resistor value in Ohms
    const V_SS: f32 = 5.3; // Supply voltage in Volts
    // Steinhart-Hart coefficients for the thermistor
    // https://docs.google.com/spreadsheets/d/1Nf47ojSvB1wB5JmTSs-cXLMhxmIMcvHLitLAx047UdE/edit?pli=1&gid=1211676988#gid=1211676988
    const A : f64 = 0.0002264321654;
    const B : f64 = 0.0003753456578;
    const C : f64 = -0.0000004022657641;

    pub fn new() -> Result<Self, Box<dyn Error>> {
        let i2c_bus = I2cdev::new("/dev/i2c-1")?;
        let mut adc = MCP342x::new(i2c_bus, 0x68);
        adc.set_channel(Channel::Ch3);
        adc.set_gain(Gain::G1);
        adc.set_resolution(Resolution::Bits16);
        adc.convert()?; // Force one shot mode and write the configuration
        std::thread::sleep(Duration::from_millis(10));
        Ok(Self { adc })
    }
    pub fn measure(&mut self) -> Option<f32> {
        let voltage = self.adc.convert_and_read(true, false).map_err(|e| {
            warn!("Thermistor measurement error: {:?}", e);
        }).ok()?;

        info!("Thermistor voltage: {}", voltage);

        // R = (voltage divider resistor [Ohms]) * (Vss [V] / voltage [V] - 1)
        let resistance: f64 = (Self::R_I * (Self::V_SS / voltage - 1.0)).into();
        // Calculate temperature in Celsius using the Steinhart-Hart equation
        let temp = 1.0 / (Self::A + Self::B*resistance.ln() + Self::C*resistance.ln().powi(3)) - 273.15;
        Some(temp as f32)
    }
}

pub struct AudioRecorder {
    pub audio_directory: String,
    pub recording_time: Duration,
    pub device_id: String
}
impl AudioRecorder {
    pub fn new(audio_directory: String, recording_time: Duration, device_id: String) -> Self {
        Self { audio_directory, recording_time, device_id }
    }
    pub async fn async_audio_recording(&self) -> Result<AudioRecording, Box<dyn Error + Send + Sync>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        let filepath = format!("{}/audio_{}.mp3", &self.audio_directory, timestamp);

        // spawn ffmpeg and wait asynchronously
        let mut child = Command::new("ffmpeg")
            .args([
                "-f", "alsa",
                "-ac", "1",
                "-i", &self.device_id,
                "-t", &self.recording_time.as_secs().to_string(),
                "-acodec", "libmp3lame",
                "-b:a", "128k",
                "-y",
                &filepath,
            ])
            .spawn()?;

        let status = child.wait().await?;
        if !status.success() {
            return Err(format!("ffmpeg exited with {:?}", status).into());
        }

        Ok(AudioRecording {
            path: filepath,
            duration: self.recording_time,
            start_time: timestamp,
        })
    }
}

pub struct SensorReader {
    bme280: BME280Wrapper,
    ens160: ENS160Wrapper,
    thermistor: ThermistorWrapper,
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

        let thermistor = ThermistorWrapper::new()?;
        info!("Thermistor ADC initialized successfully.");

        let camera = CameraWrapper::new(data_path.to_string() + "/images/" )?;            
        info!("Camera initialized successfully.");
        
        Ok(Self { bme280, ens160, thermistor, camera })
    }

    #[tracing::instrument(skip(self))]
    pub fn measure(&mut self) -> Result<SleepData, SystemTimeError> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut builder = SleepData::builder(timestamp);

        if let Some(bme280_measurements) = self.bme280.measure() {
            builder = builder.with_bme280(bme280_measurements);
        } 
        if let Some(ens160_measurements) = self.ens160.measure() {
            builder = builder.with_ens160(ens160_measurements);
        } 
        if let Some(thermistor_measurement) = self.thermistor.measure() {
            builder = builder.with_thermistor_temp(thermistor_measurement);
        }
        if let Some(image_path) = self.camera.measure(&timestamp.to_string()) {
            builder = builder.with_image_path(image_path);
        }

        Ok(builder.build())
    }
}
