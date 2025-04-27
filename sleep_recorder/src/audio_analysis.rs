use hdf5::{types::VarLenUnicode, File as H5File, H5Type};
use minimp3::{Decoder, Frame, Error as Minimp3Error};
use std::{error::Error, fs::File};
use tracing::info;

use crate::data::H5AudioMetadata;

#[tracing::instrument()]
pub fn analyze_audio_entries(data_path: &str, file_name: &str, group_name: &str) -> Result<(), Box<dyn Error>> {
    const WINDOW_SIZE_S: usize = 5;
    info!("Analyzing audio entries...");
    let file = H5File::append(data_path.to_string() + "/" + file_name)?;

    let group = file.group(group_name)?;

    let audio_dataset = group.dataset("audio")?;
    let audio_data = audio_dataset.read_1d::<H5AudioMetadata>()?;

    info!("Audio dataset shape: {:?}", audio_dataset.shape());

    for entry in audio_data {
        let audio_path = entry.path.to_string();
        let samples = decode_mp3(&audio_path)?;
        let block_rms = block_rms(samples, WINDOW_SIZE_S);
        let timestamps = (0..block_rms.len() as u64)
            .map(|i| (entry.start_time_s + i * WINDOW_SIZE_S as u64) as u64)
            .collect::<Vec<u64>>();
        info!("Processed {} samples from {}", block_rms.len(), audio_path);
    }

    Ok(())
}

#[tracing::instrument(skip(path))]
pub fn decode_mp3(path: &str) -> Result<Vec<i16>, Box<dyn Error>> {
    let mut decoder = Decoder::new(
        File::open(path)
        .map_err(|e| format!("Failed to open file: {} with error {}", path, e))?);

    let mut samples = Vec::new();

    info!("Opened file: {}", path);

    loop {
        match decoder.next_frame() {
            Ok(Frame { data, sample_rate, channels, .. }) => {
                info!("Sample rate: {}, channels: {}", sample_rate, channels);
                samples.extend_from_slice(&data);
            },
            Err(Minimp3Error::Eof) => break,
            Err(e) => panic!("{:?}", e),
        }
    }
    Ok(samples)
}

#[tracing::instrument]
pub fn block_rms(samples: Vec<i16>, window_size_s: usize) -> Vec<f32> {
    const CHUNK: usize = 2048;
    const SAMPLE_RATE: usize = 48_000;

    // let sig = dasp_signal::from_iter(samples.iter().cloned());
    // let ring_buffer  = dasp_ring_buffer::Fixed::from([0.0; CHUNK]);
    // let rms_signal= sig.rms(ring_buffer);
    // let rms_levels: Vec<f32> = rms_signal.until_exhausted().collect();
    let rms_downsample: Vec<f32> = samples
        .chunks(CHUNK)
        .map(|w| rms_normalized(w))
        .collect();
    
    let chunks_per_time: usize = (SAMPLE_RATE * window_size_s) as usize / CHUNK;
    let rms_downsample_to_window_size: Vec<f32> = rms_downsample
        .chunks(chunks_per_time)
        .map(|w| rms_normalized(w))
        .collect();
    rms_downsample_to_window_size
}

fn rms_normalized<T: Into<f32> + Copy>(samples: &[T]) -> f32 {
    (samples.iter()
        .map(|s| Into::<f32>::into(*s).powi(2))
        .sum::<f32>()
        / samples.len() as f32)
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    // Helper constant matching the chunk size in block_rms.
    const CHUNK: usize = 2048;
    const SAMPLE_RATE: usize = 48000;

    // Test the block_rms function with a simple constant signal.
    #[test]
    fn test_block_rms_constant_signal() {
        // Create a signal with CHUNK * 24 samples, all with the value 0.5.
        // This should generate exactly 24 RMS levels of 0.5 (one per chunk).
        // With window_size_s = 1, chunks_per_time = 48000 / 2048 ≈ 23.
        // Then the smoothed vector is computed over windows of 23 values:
        // There will be (24 - 23 + 1) = 2 averaged values, and each should be near 0.5.
        let num_chunks = 24;
        let samples = vec![1_i16; CHUNK * num_chunks];
        let window_size_s: usize = 1;

        let result = block_rms(samples, window_size_s);

        // We expect two smoothed RMS values.
        assert_eq!(result.len(), ((num_chunks * CHUNK) as f32 / (SAMPLE_RATE * window_size_s) as f32).ceil() as usize);

        for &val in &result {
            // Allow some epsilon error for floating point differences.
            assert!((val - 1.0).abs() < 1e-5, "Expected near 1.0, got {}", val);
        }
    }

    // Test block_rms with a ramp signal.
    #[test]
    fn test_block_rms_ramp_signal() {
        // Create a ramp signal from 0.0 to 1.0
        let total_samples = CHUNK * 25;
        let samples: Vec<i16> = (0..total_samples)
            .map(|i| i as i16)
            .collect();
        let window_size_s = 1;
        let result = block_rms(samples, window_size_s);

        // We check that result is non-empty and values are within [0.0, 1.0].
        assert!(!result.is_empty());
        for rms in result {
            assert!(rms >= 0.0 && rms <= total_samples as f32);
        }
    }
}