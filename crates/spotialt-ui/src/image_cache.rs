use fast_image_resize::images::Image as FastImage;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use parking_lot::Mutex;
use slint::{Image as SlintImage, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct RgbaRawImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RgbaRawImage {
    pub fn to_slint_image(&self) -> SlintImage {
        let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(self.width, self.height);
        let slice = pixel_buffer.make_mut_slice();
        for (i, chunk) in self.rgba.chunks_exact(4).enumerate() {
            if i < slice.len() {
                slice[i] = Rgba8Pixel {
                    r: chunk[0],
                    g: chunk[1],
                    b: chunk[2],
                    a: chunk[3],
                };
            }
        }
        SlintImage::from_rgba8(pixel_buffer)
    }
}

pub struct ImageCache {
    memory_cache: Mutex<HashMap<String, Arc<RgbaRawImage>>>,
    max_items: usize,
}

impl ImageCache {
    pub fn new(max_items: usize) -> Self {
        Self {
            memory_cache: Mutex::new(HashMap::with_capacity(max_items)),
            max_items,
        }
    }

    pub fn get(&self, url: &str) -> Option<Arc<RgbaRawImage>> {
        let cache = self.memory_cache.lock();
        cache.get(url).cloned()
    }

    pub fn insert_and_scale(
        &self,
        url: String,
        image_bytes: &[u8],
        target_width: u32,
        target_height: u32,
    ) -> Option<Arc<RgbaRawImage>> {
        // Decode image using image crate
        let img = image::load_from_memory(image_bytes).ok()?.to_rgba8();
        let (src_w, src_h) = (img.width(), img.height());

        // Fast SIMD resize
        let src_image = FastImage::from_vec_u8(src_w, src_h, img.into_raw(), PixelType::U8x4).ok()?;

        let mut dst_image = FastImage::new(target_width, target_height, PixelType::U8x4);

        let mut resizer = Resizer::new();
        resizer
            .resize(&src_image, &mut dst_image, &ResizeOptions::default())
            .ok()?;

        let raw = Arc::new(RgbaRawImage {
            width: target_width,
            height: target_height,
            rgba: dst_image.into_vec(),
        });

        let mut cache = self.memory_cache.lock();
        if cache.len() >= self.max_items {
            cache.clear(); // Keep memory strictly bounded
        }
        cache.insert(url, raw.clone());

        Some(raw)
    }
}
