use image::{GenericImageView, Rgba};
use once_cell::sync::Lazy;
use std::path::Path;

static LOGO_CACHE: Lazy<Option<Vec<String>>> = Lazy::new(|| load_pixel_art_logo().ok());

fn load_pixel_art_logo() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let icon_path = Path::new("icon/Gemini_Generated_Image_vbpnpsvbpnpsvbpn.png");

    if !icon_path.exists() {
        return Err("Icon file not found".into());
    }

    let img = image::open(icon_path)?;

    // Make it smaller - 12 characters width for better detail
    let target_width = 24;  // 24 pixels = 12 braille characters
    let ratio = target_width as f32 / img.width() as f32;
    let mut target_height = (img.height() as f32 * ratio) as u32;

    // Ensure height is multiple of 4 for braille patterns
    target_height = ((target_height + 3) / 4) * 4;

    let resized = img.resize_exact(
        target_width,
        target_height,
        image::imageops::FilterType::Nearest,  // Nearest for sharper edges
    );

    let mut lines = Vec::new();

    // Process 4 rows at a time (for braille 2x4 grid)
    for y in (0..resized.height()).step_by(4) {
        let mut line = String::new();

        for x in (0..resized.width()).step_by(2) {
            // Braille dots are arranged as:
            // (1) (4)  Row 0
            // (2) (5)  Row 1
            // (3) (6)  Row 2
            // (7) (8)  Row 3
            //
            // Bit mapping: dot1=0, dot2=1, dot3=2, dot4=3, dot5=4, dot6=5, dot7=6, dot8=7

            let mut pattern_index = 0u8;

            // Collect all 8 pixels first
            let pixels = [
                resized.get_pixel(x.min(resized.width() - 1), y.min(resized.height() - 1)),
                resized.get_pixel((x + 1).min(resized.width() - 1), y.min(resized.height() - 1)),
                resized.get_pixel(x.min(resized.width() - 1), (y + 1).min(resized.height() - 1)),
                resized.get_pixel((x + 1).min(resized.width() - 1), (y + 1).min(resized.height() - 1)),
                resized.get_pixel(x.min(resized.width() - 1), (y + 2).min(resized.height() - 1)),
                resized.get_pixel((x + 1).min(resized.width() - 1), (y + 2).min(resized.height() - 1)),
                resized.get_pixel(x.min(resized.width() - 1), (y + 3).min(resized.height() - 1)),
                resized.get_pixel((x + 1).min(resized.width() - 1), (y + 3).min(resized.height() - 1)),
            ];

            // Map pixels to braille dots
            // Grid:      Braille:
            // 0 1        1 4
            // 2 3   →    2 5
            // 4 5        3 6
            // 6 7        7 8
            let braille_map = [0, 3, 1, 4, 2, 5, 6, 7];

            for (i, &pixel_idx) in braille_map.iter().enumerate() {
                if is_pixel_significant(&pixels[pixel_idx]) {
                    pattern_index |= 1 << i;
                }
            }

            // Braille Unicode starts at U+2800
            let braille_char = char::from_u32(0x2800 + pattern_index as u32).unwrap_or(' ');
            line.push(braille_char);
        }

        lines.push(line);
    }

    Ok(lines)
}

fn is_pixel_significant(pixel: &Rgba<u8>) -> bool {
    let alpha = pixel[3];

    // Skip nearly transparent pixels
    if alpha < 128 {
        return false;
    }

    // Calculate perceived brightness using luminance formula
    let brightness = (pixel[0] as f32 * 0.299
                    + pixel[1] as f32 * 0.587
                    + pixel[2] as f32 * 0.114) as u32;

    // For a black hole: we want bright areas (accretion disk) to show
    // Higher threshold = less detail, more contrast
    brightness > 100
}

pub fn get_logo() -> Option<&'static Vec<String>> {
    LOGO_CACHE.as_ref()
}

pub fn get_logo_dimensions() -> Option<(usize, usize)> {
    LOGO_CACHE.as_ref().as_ref().map(|lines| {
        let width = lines.first().map(|l| l.len()).unwrap_or(0);
        (width, lines.len())
    })
}
