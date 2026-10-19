//! The texture asset: a QOI file, uploaded to the GPU.

use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx};
use nadir_codec::qoi::{self, ColorSpace, Image};
use nadir_core::UVec2;
use nadir_gfx::{Device, TextureDesc, TextureFormat, TextureId};

/// A texture on the GPU, loaded from a QOI file through `nadir-assets`.
///
/// It is made by [`Asset::finalize`], which needs the [`Device`] in the
/// [`FinalizeCtx`]. A QOI file marked sRGB becomes an
/// [`TextureFormat::Rgba8Srgb`] texture, and one marked linear an
/// [`TextureFormat::Rgba8`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Texture {
    id: TextureId,
    size: UVec2,
    format: TextureFormat,
}

impl Texture {
    /// The texture on the device.
    pub fn id(&self) -> TextureId {
        self.id
    }

    /// Its size in pixels.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// How its pixels are stored.
    pub fn format(&self) -> TextureFormat {
        self.format
    }
}

/// A texture decoded from its file, waiting to go to the GPU.
#[derive(Debug)]
pub struct DecodedTexture {
    label: String,
    image: Image,
}

/// The placeholder's width and height: an 8×8 checker of single texels,
/// magenta and black, starting with magenta in the top-left corner.
const PLACEHOLDER_SIZE: u32 = 8;
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// The placeholder's pixels, from the top row.
fn checker() -> Vec<u8> {
    (0..PLACEHOLDER_SIZE * PLACEHOLDER_SIZE)
        .flat_map(|i| {
            let (x, y) = (i % PLACEHOLDER_SIZE, i / PLACEHOLDER_SIZE);
            if (x + y) % 2 == 0 { MAGENTA } else { BLACK }
        })
        .collect()
}

impl Asset for Texture {
    type Decoded = DecodedTexture;
    const EXTENSIONS: &'static [&'static str] = &["qoi"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<DecodedTexture, AssetError> {
        let image = qoi::decode(bytes, ctx.limits()).map_err(AssetError::Decode)?;
        Ok(DecodedTexture {
            label: ctx.path().to_string(),
            image,
        })
    }

    fn finalize(decoded: DecodedTexture, ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        let header = &decoded.image.header;
        let format = match header.color_space {
            ColorSpace::Srgb => TextureFormat::Rgba8Srgb,
            ColorSpace::Linear => TextureFormat::Rgba8,
        };
        let size = UVec2::new(header.width, header.height);
        upload(
            ctx.get_mut::<Device>()?,
            &decoded.label,
            size,
            format,
            &decoded.image.pixels,
        )
    }

    /// An 8×8 checker of magenta and black texels.
    fn placeholder(ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        let size = UVec2::new(PLACEHOLDER_SIZE, PLACEHOLDER_SIZE);
        upload(
            ctx.get_mut::<Device>()?,
            "placeholder",
            size,
            TextureFormat::Rgba8Srgb,
            &checker(),
        )
    }
}

fn upload(
    device: &mut Device,
    label: &str,
    size: UVec2,
    format: TextureFormat,
    pixels: &[u8],
) -> Result<Texture, AssetError> {
    let id = device
        .create_texture(
            &TextureDesc {
                label,
                size,
                format,
            },
            Some(pixels),
        )
        .map_err(|error| AssetError::Invalid(error.to_string()))?;
    Ok(Texture { id, size, format })
}

#[cfg(test)]
mod tests {
    use nadir_codec::Limits;
    use nadir_codec::qoi::{Channels, Header};

    use super::*;

    fn decode(bytes: &[u8]) -> Result<DecodedTexture, AssetError> {
        Texture::decode(
            bytes,
            &DecodeCtx::new("sprites/hero.qoi", Limits::default()),
        )
    }

    #[test]
    fn a_qoi_file_decodes_with_its_path_as_label() {
        let header = Header {
            width: 2,
            height: 1,
            channels: Channels::Rgba,
            color_space: ColorSpace::Linear,
        };
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let bytes = qoi::encode(&header, &pixels).expect("an image");
        let decoded = decode(&bytes).expect("a texture");
        assert_eq!(decoded.label, "sprites/hero.qoi");
        assert_eq!(decoded.image.header, header);
        assert_eq!(decoded.image.pixels, pixels);
    }

    #[test]
    fn a_file_that_is_not_qoi_fails_to_decode() {
        assert!(matches!(decode(b"PNG"), Err(AssetError::Decode(_))));
    }

    #[test]
    fn the_placeholder_is_a_magenta_and_black_checker() {
        let pixels = checker();
        let texels: Vec<[u8; 4]> = pixels.as_chunks::<4>().0.to_vec();
        assert_eq!(texels.len(), 64);
        assert_eq!(texels[0], MAGENTA);
        assert_eq!(texels[1], BLACK);
        assert_eq!(texels[8], BLACK, "the second row starts black");
        assert_eq!(texels[9], MAGENTA);
        assert_eq!(texels.iter().filter(|&&texel| texel == MAGENTA).count(), 32);
    }
}
