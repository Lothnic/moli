use std::sync::Arc;

use krilla::image::{BitsPerComponent, CustomImage, Image, ImageColorspace};

use super::PdfError;

#[derive(Clone, Hash)]
struct ImageChannels {
    rgb: Arc<[u8]>,
    alpha: Arc<[u8]>,
    width: u32,
    height: u32,
}
impl CustomImage for ImageChannels {
    fn color_channel(&self) -> &[u8] {
        &self.rgb
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        Some(&self.alpha)
    }
    fn bits_per_component(&self) -> BitsPerComponent {
        BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> ImageColorspace {
        ImageColorspace::Rgb
    }
}

pub(super) fn from_rgba(image: &crate::RasterImage, interpolate: bool) -> Result<Image, PdfError> {
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|size| size.checked_mul(4));
    if image.width == 0 || image.height == 0 || expected != Some(image.rgba.len()) {
        return Err(PdfError::Encoding("invalid RGBA image resource".into()));
    }
    let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(image.rgba.len() / 4);
    for pixel in image.rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
    }
    Image::from_custom(
        ImageChannels {
            rgb: rgb.into(),
            alpha: alpha.into(),
            width: image.width,
            height: image.height,
        },
        interpolate,
    )
    .map_err(PdfError::Encoding)
}
