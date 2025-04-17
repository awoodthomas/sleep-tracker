#![allow(non_local_definitions)]

use std::{collections::HashMap, str::FromStr};
use std::error::Error;
use std::result::Result;

use chrono::Local;
use hdf5::{types::VarLenUnicode, File, H5Type};

use tracing::{info, warn};

use crate::AudioRecording;

use super::SleepData;

#[derive(Debug)]
enum SleepField {
    U64(fn(&SleepData) -> u64),
    U16(fn(&SleepData) -> u16),
    F32(fn(&SleepData) -> f32),
    String(fn(&SleepData) -> VarLenUnicode),
}

#[derive(H5Type, Clone, Debug)]
#[repr(C)] // important: makes memory layout compatible
struct H5AudioMetadata {
    start_time: u64,
    duration: u64,
    path: VarLenUnicode,
}

impl From<AudioRecording> for H5AudioMetadata {
    fn from(rec: AudioRecording) -> Self {
        Self {
            start_time: rec.start_time,
            // Assuming AudioRecording has a duration field.
            duration: rec.duration.as_secs(),
            path: VarLenUnicode::from_str(&rec.path).unwrap_or_default(),
        }
    }
}

#[derive(Debug)]
pub struct SleepDataLogger {
    buffer: Vec<SleepData>,
    flush_every: usize,
    file: File,
    group_name: String,
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
    fn generate_dataset<T: H5Type>(group: &hdf5::Group, name: &str) -> Result<(), Box<dyn Error>> {
        group.new_dataset_builder()
            .chunk(1024)
            .deflate(6)
            .empty::<T>()
            .shape(hdf5::SimpleExtents::resizable([0]))
            .create(name)?;
        Ok(())
    }

    pub fn new(data_path: &str, file_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::append(data_path.to_string() + "/" + file_name)?;

        let now = Local::now();
        let group_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let group = file.create_group(&group_name)?;       

        let mut data_map = HashMap::new();

        data_map.insert("timestamp", SleepField::U64(|d| d.timestamp));
        data_map.insert("temperature", SleepField::F32(|d| d.temperature));
        data_map.insert("pressure", SleepField::F32(|d| d.pressure));
        data_map.insert("humidity", SleepField::F32(|d| d.humidity));
        data_map.insert("co2eq_ppm", SleepField::U16(|d| d.co2eq_ppm));
        data_map.insert("tvoc_ppb", SleepField::U16(|d| d.tvoc_ppb));
        data_map.insert("air_quality_index", SleepField::U16(|d| d.air_quality_index));
        data_map.insert("thermistor_temp", SleepField::F32(|d| d.thermistor_temp));
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

    #[tracing::instrument(skip(self))]
    pub fn add_audio_entry(&mut self, audio_recording: AudioRecording) -> Result<(), Box<dyn Error>> {
        let group = self.file.group(&self.group_name)?;
        Ok(append_to_dataset(&group, "audio", &[H5AudioMetadata::from(audio_recording)])?)
    }

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