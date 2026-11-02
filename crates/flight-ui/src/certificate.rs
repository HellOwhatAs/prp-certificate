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
    pub fn set(&self) -> Result<flight_format::CertificateSet, String> {
        let set = flight_format::CertificateSet::from_value(&self.certificate)?;
        if self.scores.len() != set.certificates.len() {
            return Err("成绩字段无效".into());
        }
        for (c, scores) in set.certificates.iter().zip(&self.scores) {
            if c.scores.iter().map(|x| *x as u64).collect::<Vec<_>>() != *scores || c.scores[6] == 0
            {
                return Err("成绩与证明字段不一致".into());
            }
        }
        Ok(set)
    }
    pub fn combine(items: &[Verified]) -> Result<Self, String> {
        let mut entries = vec![];
        for item in items {
            entries.extend(item.set()?.certificates);
        }
        let set = flight_format::CertificateSet::new(entries)?;
        let scores = set
            .certificates
            .iter()
            .map(|c| c.scores.iter().map(|v| *v as u64).collect())
            .collect();
        Ok(Self {
            certificate: serde_json::to_value(set).map_err(|e| e.to_string())?,
            scores,
        })
    }
    pub fn parts(&self) -> Result<Vec<Self>, String> {
        self.set()?
            .certificates
            .into_iter()
            .map(|c| {
                let scores = vec![c.scores.iter().map(|v| *v as u64).collect()];
                Ok(Self {
                    certificate: serde_json::to_value(flight_format::CertificateSet::new(vec![c])?)
                        .map_err(|e| e.to_string())?,
                    scores,
                })
            })
            .collect()
    }
    pub fn question(&self) -> u64 {
        self.certificate["certificates"][0]["metadata"]["question"]
            .as_u64()
            .unwrap_or(0)
    }
    pub fn team(&self) -> &str {
        self.certificate["certificates"][0]["metadata"]["team"]
            .as_str()
            .unwrap_or("")
    }
    pub fn id(&self) -> String {
        self.set().and_then(|set| set.id()).unwrap_or_default()
    }
    pub fn score(&self) -> &[u64] {
        self.scores.first().map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn score_for(&self, q: usize) -> Option<&[u64]> {
        let entries = self.certificate["certificates"].as_array()?;
        entries
            .iter()
            .position(|c| c["metadata"]["question"].as_u64() == Some(q as u64))
            .and_then(|i| self.scores.get(i))
            .map(Vec::as_slice)
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
    let certificate =
        serde_json::to_value(flight_format::unpack_set(&packed)?).map_err(|e| e.to_string())?;
    Ok(Imported {
        certificate,
        original: Some(bytes.to_vec()),
    })
}
pub fn check_document(imported: &Imported, verified: &Verified) -> Result<(), String> {
    if flight_format::CertificateSet::from_value(&imported.certificate)? != verified.set()? {
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
/// The certificate image and verification details use the same labels, units and rounding.
pub fn score_metrics(q: usize, s: &[u64]) -> [(&'static str, String); 5] {
    let util = (s[5] * 10000 + s[6] / 2) / s[6];
    [
        ("总飞机使用时间", format!("{} 分钟", fmt(s[0]))),
        ("总架次数", format!("{} 架次", fmt(s[3]))),
        ("座位利用率", format!("{}.{:02}%", util / 100, util % 100)),
        (
            "总燃油消耗",
            format!("{}.{:01} kg", fmt(s[2] / 10), s[2] % 10),
        ),
        if q == 3 {
            ("临时任务完成", format!("{} / 160 人", s[7]))
        } else {
            ("人员在途时间", format!("{} 分钟", fmt(s[1])))
        },
    ]
}
pub fn render(item: &Verified) -> Result<Vec<u8>, String> {
    let set = item.set()?;
    let packed = flight_format::pack_set(&set)?;
    let mut p = crate::carrier::paint(&packed)?;
    let scores: [Option<Vec<u64>>; 3] =
        std::array::from_fn(|q| item.score_for(q + 1).map(|s| s.to_vec()));
    draw(&mut p, set.team(), &scores);
    let id = set.id()?;
    centered(&mut p, &id[..16], 800, 2330, 23.0, [103, 108, 103]);
    Ok(p)
}

/// A draft has no carrier and cannot be exported through export_png.
pub fn render_preview(team: &str, scores: &[Option<Vec<u64>>; 3]) -> Vec<u8> {
    let mut pixels = crate::carrier::PAPER.repeat(WIDTH as usize * HEIGHT as usize);
    draw(&mut pixels, team, scores);
    pixels
}

fn fitted(p: &mut [u8], value: &str, x: i32, baseline: i32, size: f32, width: f32, color: [u8; 3]) {
    text(
        p,
        value,
        x,
        baseline,
        size * (width / text_width(value, size).max(1.0)).min(1.0),
        color,
    );
}

fn draw(p: &mut [u8], team: &str, scores: &[Option<Vec<u64>>; 3]) {
    let ink = [41, 57, 48];
    let grey = [103, 116, 106];
    let rule = [181, 191, 173];
    outline(p, 32, 32, WIDTH - 64, HEIGHT - 64, rule);
    outline(p, 48, 48, WIDTH - 96, HEIGHT - 96, [220, 224, 211]);
    centered(p, "海上油田人员直升机运载计划编排", 800, 144, 30.0, ink);
    for (i, c) in "成绩证书".chars().enumerate() {
        text(p, &c.to_string(), 624 + i as i32 * 96, 260, 72.0, ink);
    }
    rect(p, 140, 300, 1320, 2, rule);
    let team_size = 44.0 * (1280.0 / text_width(team, 44.0).max(1.0)).min(1.0);
    centered(p, team, 800, 364, team_size, ink);
    for (q, (heading, score)) in [
        "第一问 · 出海运输",
        "第二问 · 双向运输",
        "第三问 · 多日排班",
    ]
    .iter()
    .zip(scores)
    .enumerate()
    {
        let top = 410 + q as u32 * 455;
        rect(p, 140, top, 1320, 2, rule);
        text(p, heading, 140, top as i32 + 58, 32.0, ink);
        let Some(s) = score.as_ref().filter(|s| s.len() == 9 && s[6] > 0) else {
            continue;
        };
        text(p, "总飞机使用时间", 140, top as i32 + 192, 27.0, grey);
        fitted(p, &fmt(s[0]), 140, top as i32 + 295, 88.0, 500.0, ink);
        text(p, "分钟", 140, top as i32 + 344, 25.0, grey);
        let metrics = score_metrics(q + 1, s);
        for (i, (label, value)) in metrics[1..].iter().enumerate() {
            let x = if i % 2 == 0 { 770 } else { 1130 };
            let y = top as i32 + 186 + (i / 2) as i32 * 134;
            text(p, label, x, y, 25.0, grey);
            fitted(p, value, x, y + 54, 35.0, 324.0, ink);
        }
    }
    rect(p, 140, 1790, 1320, 2, rule);
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
            certificate: serde_json::to_value(
                flight_format::CertificateSet::new(vec![flight_format::Certificate {
                    metadata: flight_format::Metadata::new(
                        1,
                        "证书测试".into(),
                        "00112233-4455-4677-8899-aabbccddeeff".into(),
                    ),
                    proof: base64::engine::general_purpose::STANDARD.encode([42; 128]),
                    scores: score,
                }])
                .unwrap(),
            )
            .unwrap(),
            scores: vec![score.iter().map(|x| *x as u64).collect()],
        }
    }
    #[test]
    fn image_roundtrip_and_invalid_file_rejection() {
        let item = item();
        let bytes = export_png(&item).unwrap();
        let imported = parse_file(&bytes).unwrap();
        assert_eq!(
            flight_format::CertificateSet::from_value(&imported.certificate).unwrap(),
            item.set().unwrap()
        );
        assert!(parse_file(b"%PDF-1.7").is_err());
        assert!(parse_file(b"{}").is_err());
        assert!(parse_file(&vec![0; LIMIT + 1]).is_err());
    }
}

#[cfg(test)]
mod combined_tests {
    use super::*;
    #[test]
    fn absent_middle_question_is_blank_and_scores_cannot_be_relabelled() {
        use base64::Engine;
        let entries = (1..=3)
            .filter(|q| *q != 2)
            .map(|q| flight_format::Certificate {
                metadata: flight_format::Metadata::new(
                    q,
                    "测试".into(),
                    flight_format::nonce(&[q as u8; 16]),
                ),
                proof: base64::engine::general_purpose::STANDARD.encode([42; 128]),
                scores: [1; 9],
            })
            .collect();
        let set = flight_format::CertificateSet::new(entries).unwrap();
        let item = Verified {
            certificate: serde_json::to_value(set).unwrap(),
            scores: vec![vec![1; 9], vec![1; 9]],
        };
        let pixels = render(&item).unwrap();
        for y in 960..1300 {
            for x in 140..1460 {
                let at = (y * WIDTH as usize + x) * 3;
                assert_eq!(&pixels[at..at + 3], &crate::carrier::PAPER);
            }
        }
        assert_eq!(
            parse_file(&export_png(&item).unwrap()).unwrap().certificate,
            item.certificate
        );
        let parts = item.parts().unwrap();
        assert_eq!(parts[0].question(), 1);
        assert_eq!(parts[1].question(), 3);
        assert_eq!(
            Verified::combine(&parts).unwrap().certificate,
            item.certificate
        );
        let mut bad = item;
        bad.scores[1][0] = 2;
        assert!(bad.set().is_err());
    }
}
