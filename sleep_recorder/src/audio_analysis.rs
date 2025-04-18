use symphonia::core::audio::AudioBufferRef;
use symphonia::default::{get_probe, get_codecs};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::codecs::DecoderOptions;

use dasp_frame::Frame;
use dasp_signal::{Signal, FromIterator};


fn decode_mp3(path: &str) -> Result<Vec<f32>, Error> {
    // Open the file.
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    
    // Let Symphonia guess the format.
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = get_probe().format(&hint, mss, &FormatOptions::default(), &Default::default())?;
    let mut format = probed.format;
    
    // Find the first audio track.
    let track = format.tracks().iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| Error::DecodeError("no audio track"))?;
    
    // Create a decoder for that track.
    let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    
    let mut samples = Vec::new();
    // Read packets and decode.
    let packet = match format.next_packet() {
        Ok(pkt) => pkt,
        Err(Error::IoError(_)) => break, // EOF
        Err(e) => return Err(e),
    };
    let decoded = decoder.decode(&packet)?;

    match decoded {
        AudioBufferRef::F32(buffer) = decoded => {
            // Convert to f32 interleaved:
            for &sample in buffer.chan(0) {
                // If samples are i16, scale to f32:
                let s = sample as f32 / i16::MAX as f32;
                samples.push(s);
            }
        }
        _ => {
            // Handle other formats if needed.
            return Err(Error::DecodeError("unsupported format"));
        }
    }
    Ok(samples)
}

fn block_rms(samples: &[f32], block_size: usize) -> Vec<f32> {
    let sig = dasp_signal::from_iter(samples.iter().cloned());
    let rms_vals: Vec<f32> = sig
        .chunks(block_size)
        .into_iter()
        .map(|chunk| {
            let sum_sq: f32 = chunk.clone().fold(0.0, |acc, s| acc + s*s);
            (sum_sq / block_size as f32).sqrt()
        })
        .collect();
}