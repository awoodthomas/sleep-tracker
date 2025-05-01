//! Module containing wrappers for various sensors used in the sleep recorder project.
//! Most sensors are encapsulated within the SensorReader struct, which is responsible for initializing and measuring data from the sensors.
//! Audio recording is handled separately, because it is "polled" at a different rate than the other sensors.

use ab_glyph::{FontArc, PxScale};
use chrono::{Local, TimeZone};
use ens160_aq::Ens160;
use image::{GrayImage, ImageFormat, RgbImage};
use mcp342x::{Channel, Gain, MCP342x, Resolution};
use tokio::process::Command;
use tracing::{info, warn};

use linux_embedded_hal::{Delay, I2cdev};
use bme280::i2c::BME280;
use rscam::{Camera, Config};

use imageproc::drawing::draw_text_mut;

use std::{error::Error, fs::File, io::BufWriter, path::Path, time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH}};

use crate::data::{AudioRecording, CameraAndMotionResult, SleepData};

/// Wrapper for the BME280 sensor, providing temperature, humidity, and pressure measurements.
pub struct BME280Wrapper {
    bme280: BME280<I2cdev>,
}
impl BME280Wrapper {
    /// Creates a new instance of `BME280Wrapper`.
    /// 
    /// # Returns
    /// 
    /// * `Result<Self, Box<dyn Error>>` - A result containing the initialized `BME280Wrapper` instance or an error.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let i2c_bus = I2cdev::new("/dev/i2c-1")?;
        let mut delay = Delay;
        let mut bme280 = BME280::new_primary(i2c_bus);
        bme280.init(&mut delay)?;
        Ok(Self { bme280 })
    }
    /// Measures and returns the current temperature, humidity, and pressure from the BME280 sensor.
    /// 
    /// # Returns
    /// 
    /// * `Option<bme280::Measurements<linux_embedded_hal::I2CError>>` - A result containing the measurements or None if an error occurs.
    pub fn measure(&mut self) -> Option<bme280::Measurements<linux_embedded_hal::I2CError>> {
        let mut delay = Delay;
        self.bme280
            .measure(&mut delay)
            .map_err(|e| warn!("BME280 measurement error: {e}"))
            .ok()
    }
}

/// Wrapper for the camera, providing image capture functionality.
pub struct CameraWrapper {
    /// The camera instance used for capturing images.
    camera: Camera,
    /// The directory where captured images will be stored.
    image_directory: String,
    /// The last image captured, used for motion analysis.
    last_image: Option<GrayImage>
}
impl CameraWrapper {
    /// Creates a new instance of `CameraWrapper`.
    ///
    /// # Arguments
    /// 
    /// * `image_directory` - A string representing the directory where captured images will be stored.
    /// 
    /// # Returns
    /// 
    /// * `Result<Self, Box<dyn Error>>` - A result containing the initialized `CameraWrapper` instance or an error.
    /// 
    /// # Errors
    /// 
    /// * Returns an error if the camera initialization fails or if the camera configuration fails.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// let camera = CameraWrapper::new("/path/to/images/".to_string())
    ///    .expect("Failed to initialize camera");
    /// ```
    /// 
    pub fn new(image_directory: &str) -> Result<Self, Box<dyn Error>> {
        let mut camera = Camera::new("/dev/video0")?;
        camera.start(&Config {
            interval: (1, 30),          
            resolution: (1280, 720),
            format: b"MJPG",             // MJPEG is widely supported
            ..Default::default()
        })?;
        std::fs::create_dir_all(image_directory)?;
        Ok(Self { camera, image_directory: image_directory.to_string(), last_image: None})
    }
    /// Captures an image from the camera and saves it to the specified directory.
    /// 
    /// # Arguments
    /// 
    /// * `timestamp` - POSIX time. Will be appended to file name
    /// 
    /// # Returns
    /// 
    /// * `Result<CameraAndMotionResult>` - A result containing the path to the saved image and difference from the last image
    /// 
    pub fn measure(&mut self, timestamp: u64) -> Result<CameraAndMotionResult, Box<dyn Error>> {
        let frame = self.camera.capture()?;
    
        let image_path = format!("{}/image_{}.jpg", self.image_directory, timestamp);
        let image = image::load_from_memory(&frame)?;
        let mut rgb_img = image.to_rgb8();

        // Add a timestamp to the image
        Self::timestamp_image_mut(&mut rgb_img, timestamp)?;
    
        // Save image
        let path = Path::new(&image_path);
        let mut file = BufWriter::new(File::create(path)?);
        rgb_img
            .write_to(&mut file, ImageFormat::Jpeg)?;

        // Measure motion since last frame
        let gray_image = image.to_luma8();
        let mut motion = None;
        if let Some(last_image) = &self.last_image {
            motion = Some(crate::image_analysis::frame_difference(&gray_image, &last_image));
        }
        self.last_image = Some(gray_image);

        return Ok(CameraAndMotionResult { image_path, motion });
    }

    fn timestamp_image_mut(image: &mut RgbImage, timestamp: u64) -> Result<(), Box<dyn Error>> {
        // Load font
        let font_data = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf")?;
        let font = FontArc::try_from_vec(font_data)?;
    
        // Format timestamp into 12-hour format
        let local_time = Local.timestamp_opt(timestamp as i64, 0).single().ok_or(format!("Could not generate local timestamp"))?;
        let formatted = local_time.format("%I:%M:%S %p").to_string();
        
        // Draw timestamp onto image
        let scale = PxScale::from(36.0);
        draw_text_mut(image, image::Rgb([255, 255, 0]), 20, 20, scale, &font, &formatted);
        Ok(())
    }
}

/// Wrapper for the ENS160 sensor, providing air quality measurements.
pub struct ENS160Wrapper {
    ens160: Ens160<I2cdev, Delay>,
}
impl ENS160Wrapper {
    /// Creates a new instance of `ENS160Wrapper`.
    /// 
    /// # Arguments
    /// 
    /// * `cal_temp` - Calibration temperature in Celsius.
    /// * `cal_humidity` - Calibration humidity in percentage.
    /// 
    /// # Returns
    /// 
    /// * `Result<Self, String>` - A result containing the initialized `ENS160Wrapper` instance or an error message.
    /// 
    /// # Errors
    /// 
    /// * Returns an error if the I2C bus initialization fails or if the ENS160 initialization fails.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// let ens160 = ENS160Wrapper::new(25.0, 50.0)
    ///    .expect("Failed to initialize ENS160");
    /// ```
    /// 
    pub fn new(cal_temp: f32, cal_humidity: f32) -> Result<Self, String> {
        let i2c_bus = I2cdev::new("/dev/i2c-1").map_err(|e| format!("I2C Initialization error: {:?}", e))?;
        let delay = Delay;
        let mut ens160 = Ens160::new_secondary_address(i2c_bus, delay);
        ens160.initialize().map_err(|e| format!("ENS160 Initialization error: {:?}", e))?;
        ens160.set_temp_rh_comp(cal_temp, (cal_humidity * 100.) as u16).map_err(|e| format!("ENS160 Initialization error: {:?}", e))?;
        std::thread::sleep(Duration::from_millis(500));  // wait for the sensor to stabilize
        Ok(Self { ens160 })
    }
    /// Measures and returns the current air quality measurements from the ENS160 sensor.
    /// 
    /// # Returns
    /// 
    /// * `Option<ens160_aq::data::Measurements>` - A result containing the measurements or None if an error occurs.
    /// 
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

/// Thermistor wrapper for MCP342x ADC, with internal voltage-temperature conversion.
pub struct ThermistorWrapper {
    /// MCP342x ADC instance for reading thermistor voltage.
    adc: MCP342x<I2cdev>,
}
impl ThermistorWrapper {
    /// Constants for thermistor voltage divider and Steinhart-Hart coefficients.
    const R_I: f32 = 3200.0; // Voltage divider resistor value in Ohms
    const V_SS: f32 = 5.3; // Supply voltage in Volts
    // Steinhart-Hart coefficients for the thermistor
    // https://docs.google.com/spreadsheets/d/1Nf47ojSvB1wB5JmTSs-cXLMhxmIMcvHLitLAx047UdE/edit?pli=1&gid=1211676988#gid=1211676988
    const A : f64 = 0.0002264321654;
    const B : f64 = 0.0003753456578;
    const C : f64 = -0.0000004022657641;

    /// Creates a new instance of `ThermistorWrapper`.
    /// 
    /// # Returns
    /// 
    /// * `Result<Self, Box<dyn Error>>` - A result containing the initialized `ThermistorWrapper` instance or an error.
    /// 
    /// # Errors
    /// 
    /// * Returns an error if the I2C bus initialization fails or if the ADC configuration fails.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// let thermistor = ThermistorWrapper::new()
    ///    .expect("Failed to initialize thermistor");
    /// ```
    /// 
    /// # Note
    /// 
    /// * The channel, voltage divider, and S-H coefficients are hardcoded for the current setup.
    /// * The ADC is set to one-shot mode, and a delay is introduced to allow for measurement stabilization.
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

/// Provides functionality to record audio using `ffmpeg`.
pub struct AudioRecorder {
    /// The directory where the recorded audio files will be stored.
    pub audio_directory: String,
    /// The duration for which the audio will be recorded.
    pub recording_time: Duration,
    /// The identifier of the audio capture device (e.g. plughw:1,0)
    pub device_id: String
}
 
impl AudioRecorder {
    /// Creates a new instance of `AudioRecorder`.
    ///
    /// # Arguments
    ///
    /// * `audio_directory` - A string representing the directory where audio files will be stored.
    /// * `recording_time` - A `Duration` representing how long the recording should last.
    /// * `device_id` - A string representing the identifier of the audio capture device.
    ///
    /// # Returns
    ///
    /// An instance of `AudioRecorder` initialized with the specified parameters.

    pub fn new(audio_directory: &str, recording_time: Duration, device_id: String) -> Result<Self, Box<dyn Error>> {
        std::fs::create_dir_all(audio_directory)?;
        Ok(Self { audio_directory: audio_directory.to_string(), recording_time, device_id })
    }
    /// Asynchronously records audio by spawning a `ffmpeg` process.
    ///
    /// This method constructs a file path using the current Unix timestamp and spawns an `ffmpeg`
    /// command that captures audio from the device specified by `device_id`. The recording is saved
    /// as an MP3 file in the specified `audio_directory`.
    ///
    /// # Returns
    ///
    /// On success, returns an `AudioRecording` instance containing the path to the recorded file,
    /// the recording duration, and the start time. If an error occurs during time retrieval, process
    /// spawning, or if `ffmpeg` exits with a non-success status, the method returns an error.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The system time is earlier than the Unix epoch.
    /// * There's an error spawning the `ffmpeg` process.
    /// * The `ffmpeg` process exits with a non-success status.
    pub async fn async_audio_recording(&self) -> Result<AudioRecording, Box<dyn Error + Send + Sync>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        let filepath = format!("{}audio_{}.mp3", &self.audio_directory, timestamp);

        // spawn ffmpeg and wait asynchronously
        let mut child = Command::new("ffmpeg")
            .args([
                "-f", "alsa",
                "-ac", "1",
                "-i", &self.device_id,
                "-t", &self.recording_time.as_secs().to_string(),
                "-ac", "1",
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
            start_time_s: timestamp,
        })
    }
}

/// Represents a collection of sensor wrappers for sleep data measurement. Only supports simultaneous polling of sensors.
pub struct SensorReader {
    /// - BME280: Used for collecting environmental measurements such as temperature and humidity.
    bme280: BME280Wrapper,
    /// - ENS160: Initialized using temperature and humidity from BME280 for gas measurements (CO2 and TVOC)
    ens160: ENS160Wrapper,
    /// - Thermistor: Utilized for ADC-based temperature measurements.
    thermistor: ThermistorWrapper,
    /// - Camera: Configured with a directory path derived from the provided data_path to store images.=
    camera: CameraWrapper,
}

impl SensorReader {
    /// Creates a new instance of SensorReader with all sensors initialized.
    ///
    /// This function initializes all relevant sensor wrappers
    /// 
    /// # Arguments
    ///
    /// * `data_path` - A string slice representing the base directory where camera images will be stored.
    ///                This path is concatenated with "/images/" for the actual camera data storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the initialization of any sensor (BME280, ENS160, Thermistor, or Camera) fails,
    /// or if a measurement cannot be successfully obtained during the setup process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let sensor_reader = SensorReader::new("/path/to/data")
    ///     .expect("Failed to initialize sensor reader");
    /// ```    
    #[tracing::instrument]
    pub fn new(data_path: &str, group_name: &str) -> Result<Self, Box<dyn Error>> {
        let mut bme280 = BME280Wrapper::new()?;
        info!("BME280 initialized successfully.");

        let bme280_measurements = bme280.measure().ok_or("Failed to read BME280 measurements.")?;
        let ens160 = ENS160Wrapper::new(bme280_measurements.temperature, bme280_measurements.humidity)?;
        info!("ENS160 initialized successfully with cal temp of {}°C and {} RH.", bme280_measurements.temperature, bme280_measurements.humidity);

        let thermistor = ThermistorWrapper::new()?;
        info!("Thermistor ADC initialized successfully.");

        let camera = CameraWrapper::new(&format!("{}/{}/images/", data_path, group_name))?;            
        info!("Camera initialized successfully.");
        
        Ok(Self { bme280, ens160, thermistor, camera })
    }

    /// Measures and returns SensorData.
    ///
    /// This function fetches the current timestamp and attempts to gather sensor readings from the initialized sensors:
    /// - BME280: Provides environmental measurements, added to SleepData if available.
    /// - ENS160: Provides environmental data based on calibrated readings, added if available.
    /// - Thermistor: Provides the temperature reading, added if available.
    /// - Camera: Captures an image and includes the image path in SleepData if the measurement is successful.
    ///
    /// Sensor measurements that return None are simply skipped, allowing partial data to be collected.
    /// The constructed SleepData encapsulates the timestamp along with all successful sensor measurements.
    ///
    /// # Returns
    ///
    /// * Ok(SleepData) - When all sensor measurements (or the available ones) are successfully collected.
    /// * Err(SystemTimeError) - If acquiring the current system time fails (e.g., if the system clock is before UNIX_EPOCH).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let mut sensor_reader = SensorReader::new("/path/to/data")
    ///     .expect("Failed to initialize sensor reader");
    /// let sleep_data = sensor_reader.measure()
    ///     .expect("Failed to collect sleep data");
    /// ```
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
        if let Ok(camera_result) = self.camera.measure(timestamp) {
            builder = builder.with_camera_result(camera_result);
        }

        Ok(builder.build())
    }
}
