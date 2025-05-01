use hdf5::{types::VarLenUnicode, File as H5File};
use image::GrayImage;
use std::error::Error;
use tracing::info;

use crate::data::SleepDataLogger;

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
    let mut motions: Vec<f32> = vec![f32::NAN; image_paths.len() as usize];
    for (index, entry) in image_paths.iter().enumerate() {
        let path = entry.to_string();
        let current_image: image::ImageBuffer<image::Luma<u8>, Vec<u8>> = image::open(&path).map_err(|e| format!("Failed to open image at {} with error {}", path, e))?.into_luma8();
        if let Some(last_image) = last_image {
            let diff = frame_difference(&current_image, &last_image);
            motions[index] = diff;
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

pub fn frame_difference(new_frame: &GrayImage, old_frame: &GrayImage) -> f32 {
    assert_eq!(new_frame.dimensions(), old_frame.dimensions());
    new_frame.pixels()
        .zip(old_frame.pixels())
        .map(|(p1, p2)| (p1[0] as f32 - p2[0] as f32).abs())
        .sum::<f32>() / (new_frame.width() * new_frame.height()) as f32
}