//! Decode validation only; the caller retains the original encoded bytes.
//! Allocation budgets cover decoded output/canvas buffers and codec Limits;
//! they do not account for every internal codec allocation.
use std::io::{self, Cursor};
use image::{AnimationDecoder, ImageDecoder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind { Gif, Jpeg }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo { pub kind: ImageKind, pub width: u32, pub height: u32, pub frames: u32 }
#[derive(Debug, Clone, Copy)]
pub struct ImageLimits {
    pub max_width: u32, pub max_height: u32, pub max_input_bytes: u64,
    pub max_frames: u32, pub max_total_pixels: u64, pub max_alloc_bytes: u64,
}
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn decode_error(error: image::ImageError) -> io::Error { invalid(&error.to_string()) }
fn dimensions(w: u32, h: u32, limits: &ImageLimits, buffers: u64) -> io::Result<u64> {
    if w == 0 || h == 0 || w > limits.max_width || h > limits.max_height { return Err(invalid("image dimensions exceed limits")); }
    let pixels = u64::from(w).checked_mul(u64::from(h)).ok_or_else(|| invalid("pixel overflow"))?;
    let bytes = pixels.checked_mul(4).and_then(|n| n.checked_mul(buffers)).ok_or_else(|| invalid("allocation overflow"))?;
    if pixels > limits.max_total_pixels || bytes > limits.max_alloc_bytes { return Err(invalid("image allocation or pixel budget exceeded")); }
    Ok(pixels)
}
pub fn validate(bytes: &[u8], limits: &ImageLimits) -> io::Result<ImageInfo> {
    if bytes.len() as u64 > limits.max_input_bytes || limits.max_frames == 0 { return Err(invalid("input or frame budget exceeded")); }
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_image_width = Some(limits.max_width);
    decoder_limits.max_image_height = Some(limits.max_height);
    decoder_limits.max_alloc = Some(limits.max_alloc_bytes);
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        // Preflight logical canvas before the decoder can allocate frame buffers.
        if bytes.len() < 10 { return Err(invalid("truncated GIF header")); }
        let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        // The compositor may hold previous, current and output RGBA canvases.
        let pixels = dimensions(w, h, limits, 3)?;
        let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
        decoder.set_limits(decoder_limits).map_err(decode_error)?;
        let mut frames = 0u32;
        let mut total = 0u64;
        for frame in decoder.into_frames() {
            let frame = frame.map_err(decode_error)?;
            frames = frames.checked_add(1).ok_or_else(|| invalid("frame overflow"))?;
            total = total.checked_add(pixels).ok_or_else(|| invalid("pixel overflow"))?;
            if frames > limits.max_frames || total > limits.max_total_pixels { return Err(invalid("animation budget exceeded")); }
            drop(frame);
        }
        if frames == 0 || bytes.last() != Some(&0x3b) { return Err(invalid("empty or unterminated GIF")); }
        Ok(ImageInfo { kind: ImageKind::Gif, width: w, height: h, frames })
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
        let (width, height) = decoder.dimensions();
        dimensions(width, height, limits, 1)?;
        if decoder.total_bytes() > limits.max_alloc_bytes { return Err(invalid("JPEG output allocation exceeds budget")); }
        decoder.set_limits(decoder_limits).map_err(decode_error)?;
        let size = usize::try_from(decoder.total_bytes()).map_err(|_| invalid("output size overflow"))?;
        let mut output = Vec::new();
        output.try_reserve_exact(size).map_err(|_| invalid("output allocation failed"))?;
        output.resize(size, 0);
        decoder.read_image(&mut output).map_err(decode_error)?;
        if !bytes.ends_with(&[0xff, 0xd9]) { return Err(invalid("unterminated JPEG")); }
        Ok(ImageInfo { kind: ImageKind::Jpeg, width, height, frames: 1 })
    } else { Err(invalid("only GIF and JPEG are supported")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> ImageLimits { ImageLimits { max_width: 8, max_height: 8, max_input_bytes: 4096, max_frames: 4, max_total_pixels: 256, max_alloc_bytes: 4096 } }
    fn gif(n: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        { let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
          for _ in 0..n { encoder.encode_frame(image::Frame::new(image::RgbaImage::from_pixel(2, 2, image::Rgba([12,34,56,255])))).unwrap(); }
        }
        bytes
    }
    #[test] fn real_images_and_limits() {
        let bytes = gif(2);
        assert_eq!(validate(&bytes, &limits()).unwrap().frames, 2);
        let mut l = limits(); l.max_frames = 1; assert!(validate(&bytes, &l).is_err());
        l = limits(); l.max_total_pixels = 7; assert!(validate(&bytes, &l).is_err());
        l = limits(); l.max_width = 1; assert!(validate(&bytes, &l).is_err());
        l = limits(); l.max_input_bytes = 1; assert!(validate(&bytes, &l).is_err());
        assert!(validate(&bytes[..bytes.len()-2], &limits()).is_err());
        assert!(validate(b"GIF89a\x02\0\x02\0;", &limits()).is_err());
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode(&[10,20,30], 1, 1, image::ExtendedColorType::Rgb8).unwrap();
        assert_eq!(validate(&jpeg, &limits()).unwrap().kind, ImageKind::Jpeg);
        assert!(validate(&jpeg[..jpeg.len()-2], &limits()).is_err());
        assert!(validate(&[255,216,255,217], &limits()).is_err());
    }
    #[test] fn corrupt_later_frame_is_rejected() {
        let mut bytes = gif(2);
        // The encoder emits no extensions: identify image descriptors by their
        // complete known rectangle header, then invalidate the second LZW code size.
        let descriptor = [0x2c, 0, 0, 0, 0, 2, 0, 2, 0];
        let starts: Vec<_> = bytes.windows(descriptor.len()).enumerate()
            .filter_map(|(i, v)| (v == descriptor).then_some(i)).collect();
        assert_eq!(starts.len(), 2);
        let packed = bytes[starts[1] + 9];
        let palette_bytes = if packed & 0x80 != 0 { 3 * (1usize << ((packed & 7) + 1)) } else { 0 };
        bytes[starts[1] + 10 + palette_bytes] = 12;
        assert!(validate(&bytes, &limits()).is_err());
    }
}
