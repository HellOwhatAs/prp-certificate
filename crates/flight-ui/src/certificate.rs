use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::sync::OnceLock;

pub const FONT: &[u8] = include_bytes!("../../../assets/NotoSansSC-Regular.otf");
pub use crate::carrier::{HEIGHT, WIDTH};
const LIMIT: usize = 16_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verified {
    pub certificate: Value,
    pub scores: Vec<Vec<u64>>,
}
impl Verified {
    pub fn question(&self) -> u64 {
        self.certificate["metadata"]["question"]
            .as_u64()
            .unwrap_or(0)
    }
    pub fn team(&self) -> &str {
        self.certificate["metadata"]["team"].as_str().unwrap_or("")
    }
    pub fn nonce(&self) -> &str {
        self.certificate["metadata"]["nonce"].as_str().unwrap_or("")
    }
    pub fn score(&self) -> &[u64] {
        self.scores.last().map(Vec::as_slice).unwrap_or(&[])
    }
}

pub struct Imported {
    pub certificate: Value,
    pub original: Option<Vec<u8>>,
}
pub fn parse_file(bytes: &[u8]) -> Result<Imported, String> {
    if bytes.len() > LIMIT {
        return Err("证书图片不能超过 16 MB".into());
    }
    let reader = || -> Result<image::ImageReader<std::io::Cursor<&[u8]>>, String> {
        let mut r = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        if !matches!(
            r.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
        ) {
            return Err("请选择当前格式的 PNG 或 JPEG 证书图片".into());
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(6000);
        limits.max_image_height = Some(6000);
        limits.max_alloc = Some(96_000_000);
        r.limits(limits);
        Ok(r)
    };
    let (w, h) = reader()?
        .into_dimensions()
        .map_err(|_| "无法读取图片尺寸")?;
    if w < 128 || h < 128 || u64::from(w) * u64::from(h) > 16_000_000 {
        return Err("证书图片尺寸不受支持，请使用清晰原图".into());
    }
    let img = reader()?
        .decode()
        .map_err(|_| "图片损坏或尺寸过大")?
        .to_rgb8();
    let packed = crate::carrier::read(&img)?;
    let certificate = flight_format::unpack(&packed)?;
    Ok(Imported {
        certificate,
        original: Some(bytes.to_vec()),
    })
}
pub fn check_document(imported: &Imported, verified: &Verified) -> Result<(), String> {
    if imported.certificate != verified.certificate {
        return Err("证明数据不匹配".into());
    }
    // The proof binds decoded claims, not arbitrary surrounding image pixels.
    Ok(())
}

fn font() -> &'static fontdue::Font {
    static FONT_INSTANCE: OnceLock<fontdue::Font> = OnceLock::new();
    FONT_INSTANCE.get_or_init(|| {
        fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).expect("bundled font")
    })
}
fn rect(pixels: &mut [u8], x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
    for yy in y..(y + h).min(HEIGHT) {
        for xx in x..(x + w).min(WIDTH) {
            let i = ((yy * WIDTH + xx) * 3) as usize;
            pixels[i..i + 3].copy_from_slice(&color);
        }
    }
}
fn text(pixels: &mut [u8], value: &str, x: i32, baseline: i32, size: f32, color: [u8; 3]) {
    let mut pen = x as f32;
    for c in value.chars() {
        let (m, mask) = font().rasterize(c, size);
        let left = pen.round() as i32 + m.xmin;
        let top = baseline - m.ymin - m.height as i32;
        for yy in 0..m.height {
            for xx in 0..m.width {
                let px = left + xx as i32;
                let py = top + yy as i32;
                if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                    continue;
                }
                let a = mask[yy * m.width + xx] as u32;
                let i = ((py as u32 * WIDTH + px as u32) * 3) as usize;
                for k in 0..3 {
                    pixels[i + k] = ((color[k] as u32 * a + pixels[i + k] as u32 * (255 - a) + 127)
                        / 255) as u8;
                }
            }
        }
        pen += m.advance_width;
    }
}
fn text_width(value: &str, size: f32) -> f32 {
    value
        .chars()
        .map(|c| font().metrics(c, size).advance_width)
        .sum()
}
fn centered(pixels: &mut [u8], value: &str, cx: i32, baseline: i32, size: f32, color: [u8; 3]) {
    text(
        pixels,
        value,
        cx - (text_width(value, size) / 2.0).round() as i32,
        baseline,
        size,
        color,
    );
}
fn outline(pixels: &mut [u8], x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
    rect(pixels, x, y, w, 2, color);
    rect(pixels, x, y + h - 2, w, 2, color);
    rect(pixels, x, y, 2, h, color);
    rect(pixels, x + w - 2, y, 2, h, color);
}
pub fn fmt(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    s.chars()
        .enumerate()
        .flat_map(|(i, c)| {
            if i > 0 && (len - i) % 3 == 0 {
                vec![',', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
pub fn render(item: &Verified) -> Result<Vec<u8>, String> {
    let q = item.question();
    let s = item.score();
    if !(1..=3).contains(&q)
        || s.len() != 9
        || item.scores.len() != 1
        || item
            .scores
            .iter()
            .any(|s| s.len() != 9 || s.iter().any(|v| *v > u32::MAX as u64))
        || s[6] == 0
        || item.team().chars().count() > 40
    {
        return Err("成绩字段无效".into());
    }
    let packed = flight_format::pack(&item.certificate)?;
    let encoded: flight_format::Certificate =
        serde_json::from_value(item.certificate.clone()).map_err(|e| e.to_string())?;
    if encoded.scores.iter().map(|x| *x as u64).collect::<Vec<_>>() != s {
        return Err("成绩与证明字段不一致".into());
    }
    let mut p = crate::carrier::paint(&packed)?;
    let ink = [41, 47, 46];
    let grey = [103, 108, 103];
    let rule = [181, 183, 173];
    outline(&mut p, 32, 32, 1536, 1216, rule);
    outline(&mut p, 48, 48, 1504, 1184, rule);
    centered(
        &mut p,
        "海上油田人员直升机运载计划编排",
        800,
        132,
        26.0,
        ink,
    );
    for (i, c) in "成绩证书".chars().enumerate() {
        text(&mut p, &c.to_string(), 676 + i as i32 * 64, 230, 56.0, ink);
    }
    let label = ["", "出海运输", "双向运输", "多日排班"][q as usize];
    centered(&mut p, &format!("问题 {q} · {label}"), 800, 282, 22.0, grey);
    rect(&mut p, 360, 318, 880, 2, rule);
    let team_size = 38.0 * (1320.0 / text_width(item.team(), 38.0).max(1.0)).min(1.0);
    centered(&mut p, item.team(), 800, 394, team_size, ink);
    centered(&mut p, "总飞机使用时间", 800, 454, 22.0, grey);
    let value = fmt(s[0]);
    let vw = text_width(&value, 86.0);
    let uw = text_width("分钟", 22.0);
    let left = 800 - ((vw + 18.0 + uw) / 2.0).round() as i32;
    text(&mut p, &value, left, 550, 86.0, ink);
    text(
        &mut p,
        "分钟",
        left + vw.round() as i32 + 18,
        550,
        22.0,
        grey,
    );
    rect(&mut p, 140, 610, 1320, 2, rule);
    let util = (s[5] * 10000 + s[6] / 2) / s[6];
    let pairs = [
        ("总架次数", fmt(s[3])),
        ("座位利用率", format!("{}.{:02}%", util / 100, util % 100)),
        (
            "总燃油消耗",
            format!("{}.{:01} kg", fmt(s[2] / 10), s[2] % 10),
        ),
        (
            if q == 3 {
                "临时任务完成"
            } else {
                "人员在途时间"
            },
            if q == 3 {
                format!("{} / 160 人", s[7])
            } else {
                format!("{} 分钟", fmt(s[1]))
            },
        ),
    ];
    for (i, (label, value)) in pairs.iter().enumerate() {
        let x = 290 + i as i32 * 340;
        centered(&mut p, label, x, 662, 20.0, grey);
        let size = 30.0 * (310.0 / text_width(value, 30.0).max(1.0)).min(1.0);
        centered(&mut p, value, x, 716, size, ink);
    }
    rect(&mut p, 140, 765, 1320, 2, rule);
    centered(
        &mut p,
        &format!("证书编号  {}", item.nonce().get(..8).unwrap_or("")),
        800,
        1210,
        18.0,
        grey,
    );
    Ok(p)
}

pub fn export_png(item: &Verified) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut bytes = vec![];
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        &render(item)?,
        WIDTH,
        HEIGHT,
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> Verified {
        use base64::Engine;
        let score = [12345, 13000, 100019, 42, 80, 12000, 24000, 0, 0];
        Verified {
            certificate: serde_json::to_value(flight_format::Certificate {
                metadata: flight_format::Metadata::new(
                    1,
                    "证书测试".into(),
                    "00112233-4455-4677-8899-aabbccddeeff".into(),
                ),
                proof: base64::engine::general_purpose::STANDARD.encode([42; 128]),
                scores: score,
            })
            .unwrap(),
            scores: vec![score.iter().map(|x| *x as u64).collect()],
        }
    }
    #[test]
    fn image_roundtrip_and_old_format_rejection() {
        let item = item();
        let bytes = export_png(&item).unwrap();
        let imported = parse_file(&bytes).unwrap();
        assert_eq!(imported.certificate, item.certificate);
        assert!(parse_file(b"%PDF-1.7").is_err());
        assert!(parse_file(b"{}").is_err());
        assert!(parse_file(&vec![0; LIMIT + 1]).is_err());
    }
}
