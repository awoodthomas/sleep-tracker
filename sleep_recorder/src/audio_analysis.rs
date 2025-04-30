use hdf5::{File as H5File, types::VarLenArray};
use minimp3::{Decoder, Frame, Error as Minimp3Error};
use std::{error::Error, fs::File};
use tracing::info;

use crate::data::H5AudioMetadata;

/// Analyzes audio entries in an HDF5 file.
/// 
/// This function reads audio data from an HDF5 file, decodes the audio files, computes the volume in dBFS,
/// and updates the HDF5 file with the computed volume and timestamps.
///
/// # Arguments
/// * `data_path` - The path to the directory containing the HDF5 file.
/// * `file_name` - The name of the HDF5 file.
/// * `group_name` - The name of the group in the HDF5 file containing the audio dataset.
///
/// # Example
/// ```
/// let result = analyze_audio_entries("/path/to/data", "sleep_data.h5", "2025-04-28_09-19-00").expect("Failed to analyze audio entries");
/// ```
/// 
/// # Errors
/// 
/// If any of the following operations fail, an error is returned:
/// * Opening the HDF5 file.
/// * Reading the audio dataset.
/// * Decoding the audio files.
/// * Writing the computed volume and timestamps back to the HDF5 file.
///
#[tracing::instrument()]
pub fn analyze_audio_entries(data_path: &str, file_name: &str, group_name: &str) -> Result<(), Box<dyn Error>> {
    const WINDOW_SIZE_S: usize = 5;
    info!("Analyzing audio entries...");
    let file = H5File::append(data_path.to_string() + "/" + file_name)?;

    let group = file.group(group_name)?;

    let audio_dataset = group.dataset("audio")?;
    let audio_data = audio_dataset.read_1d::<H5AudioMetadata>()?;

    info!("Audio dataset shape: {:?}, size: {:?}", audio_dataset.shape(), audio_dataset.size());


    for (index, entry) in audio_data.iter().enumerate() {
        let audio_path: String = entry.path.to_string();
        let samples = decode_mp3(&audio_path)?;
        let volume_db = window_volume_dbfs(samples, WINDOW_SIZE_S);
        let timestamps = (0..volume_db.len() as u64)
            .map(|i| (entry.start_time_s + i * WINDOW_SIZE_S as u64) as u64)
            .collect::<Vec<u64>>();
        info!("Processed {} samples from {}", volume_db.len(), audio_path);
        info!("Timestamps: {:?}", timestamps);
        info!("Volume dB: {:?}", volume_db);
        let updated_entry = H5AudioMetadata {
            audio_rms_db: VarLenArray::from_slice(&volume_db),
            audio_rms_t_s: VarLenArray::from_slice(&timestamps),
            ..entry.clone()
        };

        audio_dataset.write_slice(&[updated_entry], (index..index+1,))?;
    }

    Ok(())
}

/// Decodes an MP3 file and returns the audio samples as a vector of i16.
/// 
/// This function uses the `minimp3` crate to decode the MP3 file.
///
/// # Arguments
/// * `path` - The path to the MP3 file.
///
/// # Example
/// ```
/// let samples = decode_mp3("/path/to/audio.mp3").expect("Failed to decode MP3 file");
/// ```
#[tracing::instrument(skip(path))]
pub fn decode_mp3(path: &str) -> Result<Vec<i16>, Box<dyn Error>> {
    let mut decoder = Decoder::new(
        File::open(path)
        .map_err(|e| format!("Failed to open file: {} with error {}", path, e))?);

    let mut samples = Vec::new();

    info!("Opened file: {}", path);

    loop {
        match decoder.next_frame() {
            Ok(Frame { data, .. }) => {
                // info!("Sample rate: {}, channels: {}", sample_rate, channels);
                samples.extend_from_slice(&data);
            },
            Err(Minimp3Error::Eof) => break,
            Err(e) => panic!("{:?}", e),
        }
    }
    Ok(samples)
}

/// Computes the RMS volume in dBFS for a given window size.
/// 
/// This function takes a vector of audio samples and computes the RMS volume in dBFS.
/// It first normalizes the samples, then computes the RMS for each chunk of audio data.
/// Finally, it computes the dBFS for each window of audio data. Assumes a sample rate of 48kHz.
/// Only complete windows are considered (e.g. for a 31s recording & 5s windows, only 6 windows are returned).
///
/// # Arguments
/// * `samples` - A vector of audio samples.
/// * `window_size_s` - The size of the window in seconds.
///
/// # Example
/// ```
/// let samples = vec![1_i16, 2_i16, 3_i16];
/// let window_size_s = 1;
/// let volume_db = window_volume_dbfs(samples, window_size_s);
/// ```
#[tracing::instrument]
fn window_volume_dbfs(samples: Vec<i16>, window_size_s: usize) -> Vec<f32> {
    const CHUNK: usize = 2048;
    const SAMPLE_RATE: usize = 48_000;

    let normalized_samples = samples.iter().map(|s| *s as f32 / i16::MAX as f32).collect::<Vec<f32>>();

    let rms_downsample: Vec<f32> = normalized_samples
        .chunks(CHUNK)
        .map(|w| rms_normalized(w))
        .collect();
    
    let chunks_per_time: usize = (SAMPLE_RATE * window_size_s) as usize / CHUNK;
    let db_windows: Vec<f32> = rms_downsample
        .chunks(chunks_per_time)
        .filter(|w| w.len() == chunks_per_time) // Throw out incomplete chunks
        .map(|w| 20.0_f32 * rms_normalized(w).log10())
        .collect();
    db_windows
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

    // Test the window_volume_db function with a simple constant signal.
    #[test]
    fn test_window_volume_db_constant_signal() {
        // Create a signal with CHUNK * 24 samples, all with the value 0.5.
        // This should generate exactly 24 RMS levels of 0.5 (one per chunk).
        // With window_size_s = 1, chunks_per_time = 48000 / 2048 ≈ 23.
        // Then the smoothed vector is computed over windows of 23 values:
        // There will be (24 - 23 + 1) = 2 averaged values, and each should be near 0.5.
        let num_chunks = 24;
        let samples = vec![1_i16; CHUNK * num_chunks];
        let window_size_s: usize = 1;

        let result = window_volume_dbfs(samples, window_size_s);

        // We expect two smoothed RMS values.
        assert_eq!(result.len(), ((num_chunks * CHUNK) as f32 / (SAMPLE_RATE * window_size_s) as f32).ceil() as usize);

        for &val in &result {
            // Allow some epsilon error for floating point differences.
            assert!((val - 1.0).abs() < 1e-5, "Expected near 1.0, got {}", val);
        }
    }

    // Test block_rms with a ramp signal.
    #[test]
    fn test_window_volume_db_ramp_signal() {
        // Create a ramp signal from 0.0 to 1.0
        let total_samples = CHUNK * 25;
        let samples: Vec<i16> = (0..total_samples)
            .map(|i| i as i16)
            .collect();
        let window_size_s = 1;
        let result = window_volume_dbfs(samples, window_size_s);

        // We check that result is non-empty and values are within [0.0, 1.0].
        assert!(!result.is_empty());
        for rms in result {
            assert!(rms >= 0.0 && rms <= total_samples as f32);
        }
    }
}