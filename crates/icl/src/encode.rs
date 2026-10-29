//! Turning icon sources into JPEGs in ICL record layout.

use image::{DynamicImage, GenericImageView};
use jpeg_encoder::{ChromaSubsamplingMethod, ColorType, Encoder, SamplingFactor};

use crate::builder::IconSource;
use crate::error::{Error, Result};
use crate::jpeg::{self, PreparedJpeg};
use crate::options::{IclOptions, OversizePolicy, Subsampling};

/// A prepared icon and the JPEG quality it was encoded at (`None` when the
/// source JPEG was stored unchanged).
pub(crate) struct Encoded {
    pub jpeg: PreparedJpeg,
    pub quality: Option<u8>,
}

pub(crate) fn encode_source(source: &IconSource, options: &IclOptions) -> Result<Encoded> {
    let limit = options.chip.max_jpeg_size();
    match source {
        IconSource::File(path) => {
            let bytes = std::fs::read(path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            encode_bytes(&bytes, options, limit)
        }
        IconSource::Encoded(bytes) => encode_bytes(bytes, options, limit),
        IconSource::Image(image) => encode_image(image, options, limit),
        IconSource::Jpeg(bytes) => Ok(Encoded {
            jpeg: jpeg::prepare(bytes, limit)?,
            quality: None,
        }),
    }
}

fn encode_bytes(bytes: &[u8], options: &IclOptions, limit: usize) -> Result<Encoded> {
    if options.passthrough_jpeg && bytes.starts_with(&[0xFF, 0xD8]) {
        // Layouts the T5L cannot decode, or that are too large, get re-encoded.
        if let Ok(jpeg) = jpeg::prepare(bytes, limit) {
            return Ok(Encoded {
                jpeg,
                quality: None,
            });
        }
    }
    encode_image(&image::load_from_memory(bytes)?, options, limit)
}

/// Encodes `image`, retrying at lower quality per [`IclOptions::oversize`]
/// while the JPEG exceeds `limit` bytes.
fn encode_image(image: &DynamicImage, options: &IclOptions, limit: usize) -> Result<Encoded> {
    let (width, height) = image.dimensions();
    let too_large = || Error::ImageTooLarge { width, height };
    let w = u16::try_from(width).map_err(|_| too_large())?;
    let h = u16::try_from(height).map_err(|_| too_large())?;
    let rgb = flatten(image, options.background);
    let step = match options.oversize {
        OversizePolicy::Error => 0,
        OversizePolicy::ReduceQuality { step } => step.max(1),
    };

    let mut quality = options.quality.clamp(1, 100);
    loop {
        let jpeg = encode_rgb(&rgb, w, h, quality, options.subsampling)?;
        match jpeg::prepare(&jpeg, limit) {
            Err(Error::IconTooLarge { .. }) if step > 0 && quality > 1 => {
                quality = quality.saturating_sub(step).max(1);
            }
            result => {
                return result.map(|jpeg| Encoded {
                    jpeg,
                    quality: Some(quality),
                });
            }
        }
    }
}

/// Returns packed RGB8 pixels, blending any alpha onto `background`.
fn flatten(image: &DynamicImage, background: [u8; 3]) -> Vec<u8> {
    if !image.color().has_alpha() {
        return image.to_rgb8().into_raw();
    }
    let rgba = image.to_rgba8();
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.pixels() {
        let alpha = u32::from(pixel[3]);
        for (channel, bg) in pixel.0[..3].iter().zip(background) {
            let blended = u32::from(*channel) * alpha + u32::from(bg) * (255 - alpha);
            rgb.push(((blended + 127) / 255) as u8);
        }
    }
    rgb
}

fn encode_rgb(
    rgb: &[u8],
    width: u16,
    height: u16,
    quality: u8,
    subsampling: Subsampling,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, quality);
    encoder.set_sampling_factor(match subsampling {
        Subsampling::S444 => SamplingFactor::F_1_1,
        Subsampling::S420 => SamplingFactor::F_2_2,
    });
    encoder.set_chroma_subsampling_method(ChromaSubsamplingMethod::Average);
    encoder.encode(rgb, width, height, ColorType::Rgb)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn flatten_blends_alpha_onto_background() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(3, 1, |x, _| match x {
            0 => Rgba([10, 20, 30, 255]),
            1 => Rgba([10, 20, 30, 0]),
            _ => Rgba([0, 0, 0, 128]),
        }));
        assert_eq!(
            flatten(&image, [200, 100, 50]),
            [10, 20, 30, 200, 100, 50, 100, 50, 25]
        );
    }

    fn stripes(size: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(size, size, |x, y| {
            image::Rgb(if (x / 2 + y / 3) % 2 == 0 {
                [250, 10, 40]
            } else {
                [5, 200, 90]
            })
        }))
    }

    #[test]
    fn reduce_quality_retries_until_it_fits() {
        let image = stripes(64);
        let rgb = flatten(&image, [255; 3]);
        let size_at = |quality| {
            encode_rgb(&rgb, 64, 64, quality, Subsampling::S420)
                .unwrap()
                .len()
        };
        let limit = size_at(60);
        let encode = |oversize| {
            let options = IclOptions {
                quality: 90,
                oversize,
                ..IclOptions::default()
            };
            encode_image(&image, &options, limit)
        };

        assert!(matches!(
            encode(OversizePolicy::Error),
            Err(Error::IconTooLarge { .. })
        ));
        // Step 1 settles on the highest quality that fits.
        let quality = encode(OversizePolicy::reduce_quality())
            .unwrap()
            .quality
            .unwrap();
        assert!(size_at(quality) <= limit);
        assert!(
            (quality + 1..=90).all(|q| size_at(q) > limit),
            "quality {quality}"
        );
        // Larger steps try 90, 80, 70, ...
        let quality = encode(OversizePolicy::ReduceQuality { step: 10 })
            .unwrap()
            .quality;
        let expected = (60..=90).rev().step_by(10).find(|&q| size_at(q) <= limit);
        assert_eq!(quality, expected);
    }

    #[test]
    fn reduce_quality_fails_when_quality_1_is_too_large() {
        let image = stripes(16);
        for step in [0, 1, 7, 255] {
            let options = IclOptions {
                oversize: OversizePolicy::ReduceQuality { step },
                ..IclOptions::default()
            };
            assert!(matches!(
                encode_image(&image, &options, 100),
                Err(Error::IconTooLarge { .. })
            ));
        }
    }
}
