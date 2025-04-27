//! This module handles the logging of sleep data to an HDF5 file.
//! It defines the `SleepDataLogger` struct, which is responsible for creating
//! the HDF5 file, defining the datasets, and appending data to them.
//! The `SleepDataLogger` struct also handles the conversion of `AudioRecording`
//! instances to HDF5-compatible metadata.
//! 
//! This also defines the `SleepData`` and `AudioRecording`` structs, which represent
//! the data entries for sleep and audio recordings, respectively.
//!

#![allow(non_local_definitions)]

use std::time::Duration;
use std::{collections::HashMap, str::FromStr};
use std::error::Error;
use std::result::Result;

use chrono::Local;
use hdf5::types::VarLenArray;
use hdf5::{types::VarLenUnicode, File, H5Type};

use tracing::{info, warn};

/// Data entry for a sleep recording session. Uses a builder pattern for construction.
#[derive(Debug)]
pub struct SleepData {
    /// Timestamp of the data entry in seconds since UNIX epoch.
    pub timestamp_s: u64,
    /// Ambient temperature in degrees Celsius.
    pub temperature_c: f32,
    /// Ambient pressure in hPa. Currently not functional.
    pub pressure: f32,
    /// Ambient humidity in percent RH.
    pub humidity: f32,
    /// Equivalent CO2 concentration in ppm.
    pub co2eq_ppm: u16,
    /// Total volatile organic compounds in ppb.
    pub tvoc_ppb: u16,
    /// Air quality index (AQI).
    pub air_quality_index: u16,
    /// Thermistor temperature in degrees Celsius.
    pub thermistor_temp_c: f32,
    /// Path to the image file.
    pub image_path: String,
}
impl SleepData {
    /// Creates a new `SleepDataBuilder` instance with the given timestamp.
    pub fn builder(timestamp: u64) -> SleepDataBuilder {
        SleepDataBuilder::new(timestamp)
    }
}

/// Builder for `SleepData`. 
/// 
/// This struct is used to construct a `SleepData` instance using a builder pattern. 
/// It allows for optional fields to be set, and provides a method to build the 
/// final `SleepData` instance. Float fields default to `NAN`, and integer fields 
/// default to `0`. The image path defaults to an empty string.
#[derive(Default)]
pub struct SleepDataBuilder {
    timestamp_s: u64,
    temperature_c: Option<f32>,
    pressure: Option<f32>,
    humidity: Option<f32>,
    co2eq_ppm: Option<u16>,
    tvoc_ppb: Option<u16>,
    air_quality_index: Option<u16>,
    thermistor_temp_c: Option<f32>,
    image_path: Option<String>,
}

impl SleepDataBuilder {
    pub fn new(timestamp: u64) -> Self {
        Self {
            timestamp_s: timestamp,
            ..Self::default()
        }
    }

    pub fn with_bme280(mut self, measurements: bme280::Measurements<linux_embedded_hal::I2CError>) -> Self {
        self.temperature_c = Some(measurements.temperature);
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
        self.thermistor_temp_c = Some(thermistor_temp);
        self
    }

    pub fn build(self) -> SleepData {
        SleepData {
            timestamp_s: self.timestamp_s,
            temperature_c: self.temperature_c.unwrap_or(f32::NAN),
            pressure: self.pressure.unwrap_or(f32::NAN),
            humidity: self.humidity.unwrap_or(f32::NAN),
            co2eq_ppm: self.co2eq_ppm.unwrap_or_default(),
            tvoc_ppb: self.tvoc_ppb.unwrap_or_default(),
            air_quality_index: self.air_quality_index.unwrap_or_default(),
            thermistor_temp_c: self.thermistor_temp_c.unwrap_or(f32::NAN),
            image_path: self.image_path.unwrap_or_default(),
        }
    }
}

/// Data entry for an audio recording session.
#[derive(Debug)]
pub struct AudioRecording {
    /// Path to the audio file.
    pub path: String,
    /// Duration of the audio recording.
    pub duration: Duration,
    /// Timestamp of the audio recording in seconds since UNIX epoch.
    pub start_time_s: u64,
}
/// HDF5-compatible metadata for audio recordings. Implements `from(AudioRecording)`
#[derive(H5Type, Clone, Debug)]
#[repr(C)] // important: makes memory layout compatible
pub struct H5AudioMetadata {
    /// Timestamp of the audio recording in seconds since UNIX epoch.
    pub start_time_s: u64,
    /// Duration of the audio recording in seconds.
    pub duration_s: u64,
    /// Path to the audio file.
    pub path: VarLenUnicode,
    /// Audio RMS volume in dB.
    pub audio_rms_db: VarLenArray<f32>,
    /// RMS volume timestamps in seconds since UNIX epoch.
    pub audio_rms_t_s: VarLenArray<u64>,
}

impl From<AudioRecording> for H5AudioMetadata {
    fn from(rec: AudioRecording) -> Self {
        Self {
            start_time_s: rec.start_time_s,
            // Assuming AudioRecording has a duration field.
            duration_s: rec.duration.as_secs(),
            path: VarLenUnicode::from_str(&rec.path).unwrap_or_default(),
            audio_rms_db: VarLenArray::from_slice(&[]),
            audio_rms_t_s: VarLenArray::from_slice(&[]),
        }
    }
}

#[derive(Debug)]
enum SleepField {
    U64(fn(&SleepData) -> u64),
    U16(fn(&SleepData) -> u16),
    F32(fn(&SleepData) -> f32),
    String(fn(&SleepData) -> VarLenUnicode),
}

/// Logger for sleep data. 
/// 
/// This struct is responsible for creating the HDF5 file,
/// defining the datasets, and appending data to them. It also handles the conversion
/// of `AudioRecording` instances to HDF5-compatible metadata.
/// The logger uses a buffer to store data temporarily, and it flushes the data
/// to the HDF5 file when the buffer reaches a certain size. 
/// Implements the `Drop` trait to ensure that data is flushed to the file
/// when the logger is dropped.
#[derive(Debug)]
pub struct SleepDataLogger {
    /// Buffer for storing sleep data entries before flushing to HDF5 file.
    buffer: Vec<SleepData>,
    /// Number of entries to buffer before flushing to HDF5 file.
    flush_every: usize,
    /// HDF5 file handle.
    file: File,
    /// Name of the HDF5 group for this session.
    group_name: String,
    /// Map of dataset names to their corresponding SleepField functions.
    data_map: HashMap<&'static str, SleepField>,
}

impl Drop for SleepDataLogger {
    fn drop(&mut self) {
        info!("Data logging ended, flushing final data.");
        if let Err(e) = self.flush() {
            warn!("Failed to flush data on drop: {}", e);
        }
    }
}

impl SleepDataLogger {
    /// Creates a new HDF5 dataset for the given type and name.
    /// The dataset is created with chunking and compression enabled.
    /// The dataset is resizable and initially empty.
    fn generate_dataset<T: H5Type>(group: &hdf5::Group, name: &str) -> Result<(), Box<dyn Error>> {
        group.new_dataset_builder()
            .chunk(1024)
            .deflate(6)
            .empty::<T>()
            .shape(hdf5::SimpleExtents::resizable([0]))
            .create(name)?;
        Ok(())
    }

    /// Creates a new `SleepDataLogger` instance.
    /// The HDF5 file is created at the specified path with the given filename.
    /// A new group is created in the file with the current timestamp as its name.
    /// The datasets for the sleep data fields are created in the group.
    /// Defaults the `flush_every` parameter to 12.
    pub fn new(data_path: &str, file_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::append(data_path.to_string() + "/" + file_name)?;

        let now = Local::now();
        let group_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let group = file.create_group(&group_name)?;       

        let mut data_map = HashMap::new();

        data_map.insert("timestamp", SleepField::U64(|d| d.timestamp_s));
        data_map.insert("temperature", SleepField::F32(|d| d.temperature_c));
        data_map.insert("pressure", SleepField::F32(|d| d.pressure));
        data_map.insert("humidity", SleepField::F32(|d| d.humidity));
        data_map.insert("co2eq_ppm", SleepField::U16(|d| d.co2eq_ppm));
        data_map.insert("tvoc_ppb", SleepField::U16(|d| d.tvoc_ppb));
        data_map.insert("air_quality_index", SleepField::U16(|d| d.air_quality_index));
        data_map.insert("thermistor_temp", SleepField::F32(|d| d.thermistor_temp_c));
        data_map.insert("image_path", SleepField::String(|d| VarLenUnicode::from_str(&d.image_path).unwrap_or_default()));
    
        for (key, sleep_field) in data_map.iter() {
            match sleep_field {
                SleepField::U64(_) => Self::generate_dataset::<u64>(&group, key)?,
                SleepField::U16(_) => Self::generate_dataset::<u16>(&group, key)?,
                SleepField::F32(_) => Self::generate_dataset::<f32>(&group, key)?,
                SleepField::String(_) => Self::generate_dataset::<VarLenUnicode>(&group, key)?,
            };
        }
        Self::generate_dataset::<H5AudioMetadata>(&group, "audio")?;
        info!("HDF5 file ({file_name}) and group ({group_name}) created successfully at {data_path}.");

        Ok(Self {
            buffer: Vec::new(),
            flush_every: 12,
            file,
            group_name: group_name.to_string(),
            data_map
        })
    }

    /// Appends a new `SleepData` entry to the buffer.
    /// If the buffer reaches the specified size, it flushes the data to the HDF5 file.
    /// The `flush_every` parameter determines how many entries to buffer before flushing.
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

    /// Appends a new `AudioRecording` entry to the HDF5 file.
    #[tracing::instrument(skip(self))]
    pub fn add_audio_entry(&mut self, audio_recording: AudioRecording) -> Result<(), Box<dyn Error>> {
        let group = self.file.group(&self.group_name)?;
        Ok(append_to_dataset(&group, "audio", &[H5AudioMetadata::from(audio_recording)])?)
    }

    /// Flushes the buffered data to the HDF5 file.
    #[tracing::instrument(skip(self))]
    pub fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        let buffer = std::mem::take(&mut self.buffer);
        let file = self.file.clone();
        let group_name = self.group_name.clone();

        if buffer.is_empty() {
            return Ok(());
        }

        let group = file.group(&group_name)?;

        // Collect data from buffer
        for (name, sleep_field) in self.data_map.iter() {
            match sleep_field {
                SleepField::U64(f) => {
                    let data: Vec<u64> = buffer.iter().map(f).collect();
                    append_to_dataset(&group, name, &data)?;
                }
                SleepField::U16(f) => {
                    let data: Vec<u16> = buffer.iter().map(f).collect();
                    append_to_dataset(&group, name, &data)?;
                }
                SleepField::F32(f) => {
                    let data: Vec<f32> = buffer.iter().map(f).collect();
                    append_to_dataset(&group, name, &data)?;
                }
                SleepField::String(f) => {
                    let data: Vec<VarLenUnicode> = buffer.iter().map(f).collect();
                    append_to_dataset(&group, name, &data)?;
                }
            }
        }

        info!("Successfully flushed to hdf5");
        Ok(())
    }    
}

/// Appends new values to an existing dataset in the HDF5 file. 
/// Uses `resize` and `write_slice` to add new data.
/// # Arguments
/// * `group` - The HDF5 group containing the dataset.
/// * `dataset_name` - The name of the dataset to append to.
/// * `new_vals` - The new values to append to the dataset.
///
/// # Errors
/// If the dataset does not exist or if there is an error during resizing or writing.
fn append_to_dataset<T: H5Type>(group: &hdf5::Group, dataset_name: &str, new_vals: &[T]) -> hdf5::Result<()> {
    let dataset = group.dataset(dataset_name)?;

    // 1) find current length
    let old_len = dataset.shape()[0];
    let add_len = new_vals.len();
    let new_len = old_len + add_len;

    // 2) resize the dataset
    //    for 1‑D you can just pass the new length
    dataset.resize(new_len)?;

    // 3) write into the hyperslab at the end
    //    note the comma to make it a 1‑tuple
    dataset.write_slice(new_vals, (old_len..new_len,))?;

    Ok(())
}