 //! This module contains functions for image analysis for the sleep tracker application.

use hdf5::{types::VarLenUnicode, File as H5File};
use image::GrayImage;
use std::error::Error;
use tracing::{error, info};

use crate::data::SleepDataLogger;

/// Analyzes motion by computing differences between consecutive images stored in an HDF5 file for offline analysis.
///
/// This function opens an HDF5 file located at the given `data_path` combined with `file_name`,
/// and accesses a specific group defined by `group_name`. It then reads a dataset named "image_path"
/// to obtain the list of image file paths. For each consecutive pair of images, it computes the
/// average absolute difference in pixel intensities using the `frame_difference` function. The
/// result for each pair is stored in a vector, which is eventually written to (or used to generate)
/// the "image_motion" dataset in the same group.
///
/// Progress is logged after processing an interval of images (set by a percentage threshold).
///
/// # Arguments
///
/// * `data_path` - A string slice representing the directory path where the HDF5 file is located.
/// * `file_name` - A string slice that specifies the name of the HDF5 file.
/// * `group_name` - A string slice identifying the group within the HDF5 file containing relevant datasets.
///
/// # Returns
///
/// A `Result` which is `Ok(())` if the analysis is successful, or contains an error encapsulated in
/// a boxed dynamic error type otherwise.
///
/// # Errors
///
/// This function returns an error if:
/// - The HDF5 file or the specified group cannot be opened.
/// - The required datasets ("image_path" or "image_motion") cannot be read or generated.
/// - An image file cannot be opened or processed.
///
/// # Examples
///
/// ```no_run
/// use sleep_recorder::image_analysis::analyze_motion;
/// let result = analyze_motion("/data", "record.h5", "session1").expect("Failed to analyze motion");
/// ```
#[tracing::instrument()]
pub fn analyze_motion(data_path: &str, file_name: &str, group_name: &str) -> Result<(), Box<dyn Error>> {
    const PROGRESS_PERCENT: f32 = 0.01;
    info!("Analyzing image motion...");
    let file = H5File::append(data_path.to_string() + "/" + file_name)?;

    let group = file.group(group_name)?;

    let image_dataset = group.dataset("image_path")?;
    let image_paths = image_dataset.read_1d::<VarLenUnicode>()?;

    let motion_dataset = match group.dataset("image_motion") {
        Ok(dataset) => dataset,
        Err(_) => SleepDataLogger::generate_dataset::<f32>(&group, "image_motion")?,    
    };

    info!("Image dataset shape: {:?}, size: {:?}", image_dataset.shape(), image_dataset.size());

    let mut last_image = None;
    let mut motions: Vec<f32> = vec![f32::NAN; image_paths.len()];
    for (index, entry) in image_paths.iter().enumerate() {
        let path = entry.to_string();
        let current_image: image::ImageBuffer<image::Luma<u8>, Vec<u8>> = image::open(&path).map_err(|e| format!("Failed to open image at {} with error {}", path, e))?.into_luma8();
        if let Some(last_image) = last_image {
            let diff = frame_difference(&current_image, &last_image);
            motions[index] = diff.unwrap_or(-1.0);
        }
        last_image = Some(current_image);
        if index % (image_paths.len() as f32 * PROGRESS_PERCENT) as usize == 0 {
            info!("Progress: {:.2}%", (index as f32 / image_paths.len() as f32) * 100.0);
        }
    }
    motion_dataset.resize(image_paths.len())?;
    motion_dataset.write(&motions)?;
    Ok(())
}

/// Computes the average absolute difference of pixel intensities between two grayscale images.
///
/// The function performs a per-pixel comparison between `new_frame` and `old_frame` (both assumed to have
/// the same dimensions). It computes the absolute difference for each corresponding pixel,
/// sums these differences, and divides by the total number of pixels to obtain the average difference.
///
/// # Arguments
///
/// * `new_frame` - A reference to the new grayscale image frame.
/// * `old_frame` - A reference to the previous grayscale image frame (of identical dimensions).
///
/// # Returns
///
/// A result with a floating point number (`f32`) representing the average absolute difference per pixel,
/// or an error message if the dimensions of the images do not match.
///
/// # Examples
///
/// ```ignore
/// use sleep_recorder::image_analysis::frame_difference;
/// let diff = frame_difference(&new_gray_image, &old_gray_image);
/// println!("Frame difference: {}", diff.expect("Failed to compute frame difference"));
/// ```
pub fn frame_difference(new_frame: &GrayImage, old_frame: &GrayImage) -> Result<f32, String> {
    if new_frame.dimensions() != old_frame.dimensions() {
        let err_message: String = format!(
            "Image dimensions do not match: new {:?} vs old {:?}",
            new_frame.dimensions(),
            old_frame.dimensions()
        );
        error!(err_message);
        return Err(err_message);
    }
    Ok(new_frame.pixels()
        .zip(old_frame.pixels())
        .map(|(p1, p2)| (p1[0] as f32 - p2[0] as f32).abs())
        .sum::<f32>() / (new_frame.width() * new_frame.height()) as f32)
}