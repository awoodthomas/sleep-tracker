use std::{collections::HashMap, str::FromStr};
use std::error::Error;
use std::result::Result;

use chrono::Local;
use hdf5::{types::VarLenUnicode, File, H5Type};

use tracing::{info, warn};

use super::SleepData;

#[derive(Debug)]
enum SleepField {
    U64(fn(&SleepData) -> u64),
    F32(fn(&SleepData) -> f32),
    String(fn(&SleepData) -> VarLenUnicode),
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
        data_map.insert("image_path", SleepField::String(|d| VarLenUnicode::from_str(&d.image_path).unwrap_or_default()));
    
        for (key, sleep_field) in data_map.iter() {
            match sleep_field {
                SleepField::U64(_) => Self::generate_dataset::<u64>(&group, key)?,
                SleepField::F32(_) => Self::generate_dataset::<f32>(&group, key)?,
                SleepField::String(_) => Self::generate_dataset::<VarLenUnicode>(&group, key)?,
            };
        }
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
    pub fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        let buffer = std::mem::take(&mut self.buffer);
        let file = self.file.clone();
        let group_name = self.group_name.clone();

        if buffer.is_empty() {
            return Ok(());
        }

        let group = file.group(&group_name)?;

        // Define a helper function to handle dataset operations
        fn write_dataset<T: H5Type>(
            group: &hdf5::Group,
            dataset_name: &str,
            data: Vec<T>,
            current_size: usize,
            new_size: usize,
        ) -> Result<(), Box<dyn Error>> {
            let dataset = group.dataset(dataset_name)?;
            dataset.resize(new_size)?;
            dataset.write_slice(&data, (current_size..new_size,))?;
            Ok(())
        }

        let current_size = group.dataset("timestamp")?.shape()[0];
        let new_size = current_size + buffer.len();

        // Collect data from buffer
        for (name, sleep_field) in self.data_map.iter() {
            match sleep_field {
                SleepField::U64(f) => {
                    let data: Vec<u64> = buffer.iter().map(f).collect();
                    write_dataset(&group, name, data, current_size, new_size)?;
                }
                SleepField::F32(f) => {
                    let data: Vec<f32> = buffer.iter().map(f).collect();
                    write_dataset(&group, name, data, current_size, new_size)?;
                }
                SleepField::String(f) => {
                    let data: Vec<VarLenUnicode> = buffer.iter().map(f).collect();
                    write_dataset(&group, name, data, current_size, new_size)?;
                }
            }
        }

        info!("Successfully flushed, size increased from {current_size} to {new_size}");
        Ok(())
    }
}

