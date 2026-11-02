//! Standard binary QR carrier. Only decoded claims are authenticated by Groth16.
use image::{DynamicImage, RgbImage};
use qrcode::{Color, EcLevel, QrCode};
pub const WIDTH: u32 = 1600;
pub const HEIGHT: u32 = 1280;
pub const PAPER: [u8; 3] = [250, 249, 245];
pub fn paint(payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() > flight_format::MAX_PAYLOAD {
        return Err("证书载荷过大".into());
    }
    let code =
        QrCode::with_error_correction_level(payload, EcLevel::M).map_err(|e| e.to_string())?;
    let n = code.width();
    let scale = 4;
    let edge = (n + 8) * scale;
    let left = (WIDTH as usize - edge) / 2;
    let top = 850;
    let mut p = PAPER.repeat(WIDTH as usize * HEIGHT as usize);
    for y in 0..n {
        for x in 0..n {
            if code[(x, y)] == Color::Dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let i = ((top + (y + 4) * scale + dy) * WIDTH as usize
                            + left
                            + (x + 4) * scale
                            + dx)
                            * 3;
                        p[i..i + 3].copy_from_slice(&[41, 47, 46]);
                    }
                }
            }
        }
    }
    Ok(p)
}
pub fn read(img: &RgbImage) -> Result<Vec<u8>, String> {
    let gray = DynamicImage::ImageRgb8(img.clone()).into_luma8();
    let mut binary = gray.clone();
    for pixel in binary.pixels_mut() {
        pixel[0] = if pixel[0] < 128 { 0 } else { 255 };
    }
    let mut found: Option<Vec<u8>> = None;
    // A fixed threshold removes JPEG ringing around the high-contrast modules.
    // Also inspect the unmodified luminance for images taken under uneven light.
    for source in [binary, gray] {
        let mut prepared = rqrr::PreparedImage::prepare(source);
        for grid in prepared.detect_grids() {
            let mut bytes = Vec::new();
            if grid.decode_to(&mut bytes).is_ok() && bytes.starts_with(b"PRG1") {
                flight_format::unpack(&bytes)?;
                if found.as_ref().is_some_and(|previous| *previous != bytes) {
                    return Err("图片中包含多份证书，请分别导入".into());
                }
                found = Some(bytes);
            }
        }
    }
    found.ok_or_else(|| "未读取到当前格式的证书二维码，请使用清晰原图".into())
}
