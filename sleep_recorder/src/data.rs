use std::str::FromStr;
use std::error::Error;
use std::result::Result;

use chrono::Local;
use hdf5::{File, types::VarLenUnicode};

use tracing::{info, warn};

use super::SleepData;

#[derive(Debug)]
pub struct SleepDataLogger {
    buffer: Vec<SleepData>,
    flush_every: usize,
    file: File,
    group_name: String,
}

impl SleepDataLogger {
    pub fn new(data_path: &str, file_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::append(data_path.to_string() + "/" + file_name)?;

        let now = Local::now();
        let group_name = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let group = file.create_group(&group_name)?;       

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

