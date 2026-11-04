use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextureHandle, Vec2, pos2, vec2,
};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use std::time::Duration;

const PDF_BYTES: &[u8] = include_bytes!("../../../../public/problem.pdf");
const ACCENT: Color32 = Color32::from_rgb(27, 118, 106);
const INK: Color32 = Color32::from_rgb(29, 54, 59);
const MUTED: Color32 = Color32::from_rgb(98, 119, 119);
const PAPER_BORDER: Color32 = Color32::from_rgb(215, 224, 219);
const READER_BACKGROUND: Color32 = Color32::from_rgb(236, 242, 238);
const DEFAULT_PAGE_WIDTH: f32 = 900.0;
const CSS_PIXELS_PER_POINT: f32 = 96.0 / 72.0;
const PAGE_GAP: f32 = 18.0;
// A large zoom changes the display size without permitting an unbounded bitmap.
// Three maximum-size pages fit in the texture budget, including a nearby page.
const MAX_PAGE_PIXELS: usize = 4 * 1024 * 1024;
const MAX_TEXTURE_BYTES: usize = 48 * 1024 * 1024;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum ViewMode {
    #[default]
    Comfortable,
    FitWidth,
    Zoom(f32),
}

impl ViewMode {
    fn page_width(self, available: f32, natural_width: f32) -> f32 {
        match self {
            Self::Comfortable => available.min(DEFAULT_PAGE_WIDTH),
            Self::FitWidth => available,
            Self::Zoom(zoom) => natural_width * zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        }
        .max(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RenderKey {
    page: usize,
    width: u32,
}

struct CachedPage {
    key: RenderKey,
    texture: TextureHandle,
    bytes: usize,
    used: u64,
}

struct RenderedPage {
    key: RenderKey,
    image: Result<egui::ColorImage, String>,
}

#[derive(Clone)]
struct PageLayout {
    pages: Vec<Rect>,
    size: Vec2,
    width: f32,
}

impl PageLayout {
    fn new(dimensions: &[Vec2], width: f32, viewport_width: f32, padding: f32) -> Self {
        let widest = dimensions.iter().map(|d| d.x).fold(1.0, f32::max);
        let scale = width / widest;
        let content_width = (width + padding * 2.0).max(viewport_width);
        let mut top = padding;
        let pages = dimensions
            .iter()
            .map(|dimensions| {
                let size = *dimensions * scale;
                let rect = Rect::from_min_size(pos2((content_width - size.x) / 2.0, top), size);
                top += size.y + PAGE_GAP;
                rect
            })
            .collect();
        Self {
            pages,
            size: vec2(content_width, (top - PAGE_GAP + padding).max(1.0)),
            width,
        }
    }

    fn page_at(&self, y: f32) -> usize {
        self.pages
            .iter()
            .position(|page| y <= page.bottom() + PAGE_GAP / 2.0)
            .unwrap_or_else(|| self.pages.len().saturating_sub(1))
    }

    fn remap_offset(&self, previous: &Self, offset: Vec2, viewport_width: f32) -> Vec2 {
        let page = previous
            .page_at(offset.y)
            .min(self.pages.len().saturating_sub(1));
        let old = previous.pages[page];
        let new = self.pages[page];
        let fraction = (offset.y - old.top()) / old.height();
        let center = (offset.x + viewport_width / 2.0) / previous.size.x;
        vec2(
            (center * self.size.x - viewport_width / 2.0).max(0.0),
            (new.top() + fraction * new.height()).max(0.0),
        )
    }
}

pub(super) struct PdfViewer {
    dimensions: Result<Vec<Vec2>, String>,
    mode: ViewMode,
    current_page: usize,
    scroll: Vec2,
    layout: Option<PageLayout>,
    cache: Vec<CachedPage>,
    failures: Vec<(RenderKey, String)>,
    rasterizer: Rasterizer,
    frame: u64,
    last_width: f32,
    width_changed_at: f64,
}

impl Default for PdfViewer {
    fn default() -> Self {
        Self {
            dimensions: document_dimensions(),
            mode: ViewMode::default(),
            current_page: 0,
            scroll: Vec2::ZERO,
            layout: None,
            cache: Vec::new(),
            failures: Vec::new(),
            rasterizer: Rasterizer::default(),
            frame: 0,
            last_width: 0.0,
            width_changed_at: 0.0,
        }
    }
}

impl PdfViewer {
    pub(super) fn ui(&mut self, ui: &mut egui::Ui) {
        self.frame = self.frame.wrapping_add(1);
        self.receive_render(ui.ctx());
        let dimensions = match &self.dimensions {
            Ok(dimensions) => dimensions.clone(),
            Err(message) => {
                ui.label(RichText::new(message).color(MUTED));
                return;
            }
        };
        let padding = if ui.available_width() < 600.0 {
            6.0
        } else {
            18.0
        };
        let available_width = (ui.available_width() - 2.0 * padding).max(1.0);
        let natural_width =
            dimensions.iter().map(|d| d.x).fold(1.0, f32::max) * CSS_PIXELS_PER_POINT;
        let display_width = self.mode.page_width(available_width, natural_width);
        self.toolbar(ui, dimensions.len(), display_width / natural_width);

        egui::Frame::new().fill(READER_BACKGROUND).show(ui, |ui| {
            // Floating bars do not change the fit width as they appear/disappear.
            ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            let viewport_width = ui.available_width();
            let width = self
                .mode
                .page_width((viewport_width - padding * 2.0).max(1.0), natural_width);
            let layout = PageLayout::new(&dimensions, width, viewport_width, padding);
            if let Some(previous) = &self.layout
                && (previous.width - width).abs() > 0.5
            {
                self.scroll = layout.remap_offset(previous, self.scroll, viewport_width);
            }
            let now = ui.input(|input| input.time);
            if (width - self.last_width).abs() > 0.5 {
                self.last_width = width;
                self.width_changed_at = now;
            }
            let mut visible = Vec::new();
            let output = egui::ScrollArea::both()
                .id_salt("problem-pdf-scroll")
                .scroll_offset(self.scroll)
                .auto_shrink([false, false])
                .min_scrolled_width(0.0)
                .min_scrolled_height(0.0)
                .show_viewport(ui, |ui, viewport| {
                    ui.set_min_size(layout.size);
                    let origin = ui.min_rect().min.to_vec2();
                    self.current_page =
                        layout.page_at(viewport.top() + (viewport.height() * 0.3).min(180.0));
                    for (index, relative_rect) in layout.pages.iter().enumerate() {
                        if !relative_rect.intersects(viewport) {
                            continue;
                        }
                        visible.push(index);
                        let rect = relative_rect.translate(origin);
                        ui.painter().rect_filled(
                            rect.translate(vec2(0.0, 2.0)),
                            0,
                            Color32::from_black_alpha(10),
                        );
                        ui.painter().rect_filled(rect, 0, Color32::WHITE);
                        if let Some(cached) = self.cache.iter_mut().find(|p| p.key.page == index) {
                            cached.used = self.frame;
                            ui.painter().image(
                                cached.texture.id(),
                                rect,
                                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        } else if !self.failures.iter().any(|(key, _)| key.page == index) {
                            ui.painter().text(
                                rect.intersect(ui.clip_rect()).center(),
                                Align2::CENTER_CENTER,
                                "正在显示 PDF…",
                                FontId::proportional(14.0),
                                MUTED,
                            );
                        }
                        if let Some((_, message)) =
                            self.failures.iter().find(|(key, _)| key.page == index)
                        {
                            let retry_rect = Rect::from_center_size(
                                rect.intersect(ui.clip_rect()).center(),
                                vec2(168.0, 36.0),
                            );
                            ui.painter().rect_filled(retry_rect, 4, Color32::WHITE);
                            ui.painter().text(
                                retry_rect.center(),
                                Align2::CENTER_CENTER,
                                "显示失败 · 点击重试",
                                FontId::proportional(14.0),
                                ACCENT,
                            );
                            let retry = ui
                                .interact(
                                    retry_rect,
                                    ui.id().with(("pdf-retry", index)),
                                    Sense::click(),
                                )
                                .on_hover_text(message)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            retry.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    "重新显示 PDF",
                                )
                            });
                            if retry.clicked() {
                                self.failures.retain(|(key, _)| key.page != index);
                            }
                        }
                        ui.painter().rect_stroke(
                            rect,
                            0,
                            Stroke::new(1.0_f32, PAPER_BORDER),
                            StrokeKind::Outside,
                        );
                    }
                });
            self.scroll = output.state.offset;
            self.queue_visible_pages(ui, &layout, &dimensions, &visible, now);
            self.layout = Some(layout);
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, pages: usize, zoom: f32) {
        let frame = egui::Frame::new()
            .fill(Color32::WHITE)
            .inner_margin(Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [42.0, 30.0],
                        egui::Label::new(
                            RichText::new(format!("{} / {}", self.current_page + 1, pages))
                                .size(14.0)
                                .color(MUTED),
                        ),
                    )
                    .on_hover_text("当前页 / 总页数");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let fit = egui::Button::new(RichText::new("适合宽度").size(14.0))
                            .selected(self.mode == ViewMode::FitWidth)
                            .min_size(vec2(74.0, 30.0));
                        if ui.add(fit).clicked() {
                            self.mode = ViewMode::FitWidth;
                            self.scroll.x = 0.0;
                        }
                        if zoom_button(ui, true, zoom < MAX_ZOOM).clicked() {
                            self.mode =
                                ViewMode::Zoom(((zoom * 10.0).round() / 10.0 + 0.1).min(MAX_ZOOM));
                        }
                        ui.add_sized(
                            [43.0, 30.0],
                            egui::Label::new(
                                RichText::new(format!("{:.0}%", zoom * 100.0))
                                    .size(14.0)
                                    .color(INK),
                            ),
                        );
                        if zoom_button(ui, false, zoom > MIN_ZOOM).clicked() {
                            self.mode =
                                ViewMode::Zoom(((zoom * 10.0).round() / 10.0 - 0.1).max(MIN_ZOOM));
                        }
                    });
                });
            });
        ui.painter().hline(
            frame.response.rect.x_range(),
            frame.response.rect.bottom(),
            Stroke::new(1.0_f32, PAPER_BORDER),
        );
    }

    fn receive_render(&mut self, ctx: &egui::Context) {
        let Some(rendered) = self.rasterizer.poll() else {
            return;
        };
        match rendered.image {
            Ok(image) => {
                let bytes = image.pixels.len() * 4;
                // Keep one resolution per page. Old textures remain usable while a new
                // zoom is rendering, then are released rather than accumulating.
                self.cache.retain(|page| page.key.page != rendered.key.page);
                while self.cache.iter().map(|page| page.bytes).sum::<usize>() + bytes
                    > MAX_TEXTURE_BYTES
                {
                    let Some((index, _)) =
                        self.cache.iter().enumerate().min_by_key(|(_, p)| p.used)
                    else {
                        break;
                    };
                    self.cache.remove(index);
                }
                self.failures
                    .retain(|(key, _)| key.page != rendered.key.page);
                self.cache.push(CachedPage {
                    key: rendered.key,
                    texture: ctx.load_texture(
                        format!("problem-pdf-{}-{}", rendered.key.page, rendered.key.width),
                        image,
                        egui::TextureOptions::LINEAR,
                    ),
                    bytes,
                    used: self.frame,
                });
            }
            Err(message) => {
                self.failures
                    .retain(|(key, _)| key.page != rendered.key.page);
                self.failures.push((rendered.key, message));
            }
        }
    }

    fn queue_visible_pages(
        &mut self,
        ui: &egui::Ui,
        layout: &PageLayout,
        dimensions: &[Vec2],
        visible: &[usize],
        now: f64,
    ) {
        if self.rasterizer.busy() || visible.is_empty() {
            return;
        }
        let mut wanted = visible.to_vec();
        // Render the reading position first if a very tall window shows several pages.
        wanted.sort_by_key(|&page| page.abs_diff(self.current_page));
        if wanted.len() < 3
            && let Some(&last) = visible.last()
            && last + 1 < dimensions.len()
        {
            wanted.push(last + 1);
        }
        if wanted.len() < 3
            && let Some(&first) = visible.first()
            && first > 0
        {
            wanted.push(first - 1);
        }
        let page_budget = (MAX_TEXTURE_BYTES / 4 / wanted.len().max(1)).min(MAX_PAGE_PIXELS);
        let ppp = ui.ctx().pixels_per_point().min(3.0);
        let max_side = ui.input(|input| input.max_texture_side);
        for page in wanted {
            let key = RenderKey {
                page,
                width: raster_width(
                    layout.pages[page].width(),
                    ppp,
                    dimensions[page],
                    max_side,
                    page_budget,
                ),
            };
            let cached = self.cache.iter().find(|p| p.key.page == page);
            if cached.is_some_and(|p| p.key.width == key.width)
                || self.failures.iter().any(|(failed, _)| *failed == key)
            {
                continue;
            }
            // Resize/zoom can reuse the previous bitmap immediately. Wait briefly
            // before replacing it so dragging a window does not queue stale sizes.
            if cached.is_some() && now - self.width_changed_at < 0.16 {
                ui.ctx().request_repaint_after(Duration::from_millis(160));
                continue;
            }
            self.rasterizer.request(key, ui.ctx().clone());
            ui.ctx().request_repaint();
            break;
        }
    }
}

fn zoom_button(ui: &mut egui::Ui, plus: bool, enabled: bool) -> egui::Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(28.0, 30.0), Sense::click());
            let visuals = ui.style().interact(&response);
            if response.hovered() || response.has_focus() {
                ui.painter()
                    .rect_filled(rect.shrink(1.0), 3, visuals.weak_bg_fill);
            }
            let color = if !enabled {
                MUTED.gamma_multiply(0.5)
            } else if response.hovered() {
                ACCENT
            } else {
                INK
            };
            let center = rect.center();
            ui.painter().line_segment(
                [center - vec2(5.0, 0.0), center + vec2(5.0, 0.0)],
                Stroke::new(1.5_f32, color),
            );
            if plus {
                ui.painter().line_segment(
                    [center - vec2(0.0, 5.0), center + vec2(0.0, 5.0)],
                    Stroke::new(1.5_f32, color),
                );
            }
            response
        })
        .inner;
    let label = if plus { "放大" } else { "缩小" };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

fn document() -> Result<Pdf, String> {
    Pdf::new(std::sync::Arc::new(PDF_BYTES))
        .map_err(|_| "题目 PDF 读取失败，请保存原文件查看".to_owned())
}

fn document_dimensions() -> Result<Vec<Vec2>, String> {
    let pdf = document()?;
    let dimensions = pdf
        .pages()
        .iter()
        .map(|page| {
            let (width, height) = page.render_dimensions();
            vec2(width, height)
        })
        .collect::<Vec<_>>();
    if dimensions.is_empty()
        || dimensions
            .iter()
            .any(|d| !d.is_finite() || d.x <= 0.0 || d.y <= 0.0)
    {
        return Err("题目 PDF 页面尺寸无效，请保存原文件查看".into());
    }
    Ok(dimensions)
}

fn raster_width(
    display_width: f32,
    ppp: f32,
    dimensions: Vec2,
    max_side: usize,
    pixels: usize,
) -> u32 {
    let aspect = dimensions.y / dimensions.x;
    let maximum = (pixels as f32 / aspect)
        .sqrt()
        .min(max_side as f32)
        .min(max_side as f32 / aspect)
        .floor()
        .max(1.0);
    // Quantizing limits cache churn from subpixel layout and display scale changes.
    let desired = ((display_width * ppp).max(1.0) / 64.0).ceil() * 64.0;
    desired.min(maximum) as u32
}

fn render_page<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    key: RenderKey,
) -> Result<hayro::vello_cpu::Pixmap, String> {
    let page = pdf.pages().get(key.page).ok_or("PDF 页码无效")?;
    let (width, height) = page.render_dimensions();
    let scale = key.width as f32 / width;
    let pixel_height = (height * scale) as usize;
    if key.width == 0
        || key.width > 4096
        || pixel_height == 0
        || pixel_height > 4096
        || key.width as usize * pixel_height > MAX_PAGE_PIXELS
    {
        return Err("PDF 显示尺寸超出限制".into());
    }
    let mut pixmap = hayro::render(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    // Return opaque white paper in both workers. The map on page 2 leaves alpha
    // 253/254 after rasterization even with a white clear color. Hayro returns
    // premultiplied RGBA, so composite the remaining coverage over white here.
    for pixel in pixmap.data_as_u8_slice_mut().chunks_exact_mut(4) {
        if pixel[3] != 255 {
            let white = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(white);
            }
            pixel[3] = 255;
        }
    }
    pixmap.set_may_have_transparency(false);
    Ok(pixmap)
}

#[cfg(not(target_arch = "wasm32"))]
fn color_image(pixmap: &hayro::vello_cpu::Pixmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied(
        [usize::from(pixmap.width()), usize::from(pixmap.height())],
        pixmap.data_as_u8_slice(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Rasterizer {
    worker: Option<NativeWorker>,
    pending: Option<RenderKey>,
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeWorker {
    requests: std::sync::mpsc::Sender<RenderKey>,
    results: std::sync::mpsc::Receiver<RenderedPage>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Rasterizer {
    fn busy(&self) -> bool {
        self.pending.is_some()
    }

    fn request(&mut self, key: RenderKey, ctx: egui::Context) {
        let worker = self.worker.get_or_insert_with(|| {
            let (requests, receive) = std::sync::mpsc::channel::<RenderKey>();
            let (send, results) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let pdf = document();
                let mut cache = RenderCache::new();
                let mut cache_width = 0;
                while let Ok(key) = receive.recv() {
                    // Hayro also caches rasterized glyphs. Clear older zoom sizes,
                    // keeping a bounded working set for this fixed document.
                    if cache_width != key.width {
                        cache = RenderCache::new();
                        cache_width = key.width;
                    }
                    let image = match &pdf {
                        Ok(pdf) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            render_page(pdf, &cache, key).map(|pixmap| color_image(&pixmap))
                        }))
                        .unwrap_or_else(|_| Err("此页显示失败，请保存原文件查看".into())),
                        Err(message) => Err(message.clone()),
                    };
                    if send.send(RenderedPage { key, image }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            NativeWorker { requests, results }
        });
        let _ = worker.requests.send(key);
        self.pending = Some(key);
    }

    fn poll(&mut self) -> Option<RenderedPage> {
        let key = self.pending?;
        let worker = self.worker.as_ref()?;
        match worker.results.try_recv() {
            Ok(rendered) => {
                self.pending = None;
                Some(rendered)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.worker = None;
                Some(RenderedPage {
                    key,
                    image: Err("PDF 显示进程已停止，请保存原文件查看".into()),
                })
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct Rasterizer {
    result: std::rc::Rc<std::cell::RefCell<Option<RenderedPage>>>,
    pending: bool,
}

#[cfg(target_arch = "wasm32")]
impl Rasterizer {
    fn busy(&self) -> bool {
        self.pending
    }

    fn request(&mut self, key: RenderKey, ctx: egui::Context) {
        use wasm_bindgen::prelude::*;
        #[wasm_bindgen(js_namespace = flightBridge)]
        extern "C" {
            #[wasm_bindgen(catch, js_name = renderProblemPdfPage)]
            async fn render_problem_pdf_page_worker(
                page: u32,
                width: u32,
            ) -> Result<JsValue, JsValue>;
        }
        let result = self.result.clone();
        self.pending = true;
        wasm_bindgen_futures::spawn_local(async move {
            let image = match render_problem_pdf_page_worker(key.page as u32, key.width).await {
                Ok(value) => decode_rendered_page(&js_sys::Uint8Array::new(&value).to_vec()),
                Err(_) => Err("此页显示失败，请保存原文件查看".into()),
            };
            *result.borrow_mut() = Some(RenderedPage { key, image });
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) -> Option<RenderedPage> {
        let result = self.result.borrow_mut().take();
        if result.is_some() {
            self.pending = false;
        }
        result
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_rendered_page(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let invalid = || "PDF 显示数据无效".to_owned();
    let header = bytes.get(..8).ok_or_else(invalid)?;
    let width = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    let height = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    let pixels = width.checked_mul(height).ok_or_else(invalid)?;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || pixels > MAX_PAGE_PIXELS
        || bytes.len() != 8 + pixels * 4
    {
        return Err(invalid());
    }
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [width, height],
        &bytes[8..],
    ))
}

// This export is called in the dedicated web worker, never by the UI thread.
// It renders only the bundled document, with the same bounds as the native worker.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn render_problem_pdf_page(page: u32, width: u32) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    thread_local! {
        static PDF: Result<Pdf, String> = document();
    }
    PDF.with(|pdf| {
        let pdf = pdf
            .as_ref()
            .map_err(|message| wasm_bindgen::JsValue::from_str(message))?;
        let cache = RenderCache::new();
        let pixmap = render_page(
            pdf,
            &cache,
            RenderKey {
                page: page as usize,
                width,
            },
        )
        .map_err(|message| wasm_bindgen::JsValue::from_str(&message))?;
        let mut bytes = Vec::with_capacity(8 + pixmap.data_as_u8_slice().len());
        bytes.extend(u32::from(pixmap.width()).to_le_bytes());
        bytes.extend(u32::from(pixmap.height()).to_le_bytes());
        bytes.extend_from_slice(pixmap.data_as_u8_slice());
        Ok(bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comfortable_width_does_not_limit_explicit_zoom_or_fit() {
        assert_eq!(ViewMode::Comfortable.page_width(1500.0, 816.0), 900.0);
        assert_eq!(ViewMode::Comfortable.page_width(324.0, 816.0), 324.0);
        assert_eq!(ViewMode::FitWidth.page_width(1500.0, 816.0), 1500.0);
        assert_eq!(ViewMode::Zoom(2.0).page_width(1500.0, 816.0), 1632.0);
    }

    #[test]
    fn resizing_keeps_the_reading_position_on_the_same_page() {
        let dimensions = vec![vec2(612.0, 792.0); 9];
        let previous = PageLayout::new(&dimensions, 900.0, 1400.0, 18.0);
        let next = PageLayout::new(&dimensions, 320.0, 332.0, 6.0);
        let offset = vec2(
            0.0,
            previous.pages[5].top() + previous.pages[5].height() * 0.4,
        );
        let mapped = next.remap_offset(&previous, offset, 332.0);
        assert_eq!(next.page_at(mapped.y), 5);
        assert!((mapped.y - next.pages[5].top() - next.pages[5].height() * 0.4).abs() < 0.01);
    }

    #[test]
    fn raster_resolution_and_worker_results_are_bounded() {
        let dimensions = vec2(612.0, 792.0);
        let width = raster_width(10_000.0, 4.0, dimensions, 4096, MAX_PAGE_PIXELS);
        assert!(
            (width as f32 * width as f32 * dimensions.y / dimensions.x) <= MAX_PAGE_PIXELS as f32
        );
        assert!(decode_rendered_page(&[]).is_err());
        assert!(decode_rendered_page(&[255; 8]).is_err());
        let mut bytes = Vec::from(1u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([255, 255, 255, 255]);
        assert_eq!(decode_rendered_page(&bytes).unwrap().size, [1, 1]);
        bytes.push(0);
        assert!(decode_rendered_page(&bytes).is_err());
    }

    #[test]
    fn bundled_pdf_renders_all_nine_pages_with_original_dimensions() {
        let pdf = document().unwrap();
        let cache = RenderCache::new();
        assert_eq!(pdf.pages().len(), 9);
        for (page, original) in pdf.pages().iter().enumerate() {
            assert_eq!(original.render_dimensions(), (612.0, 792.0));
            let pixmap = render_page(&pdf, &cache, RenderKey { page, width: 612 }).unwrap();
            assert_eq!((pixmap.width(), pixmap.height()), (612, 792));
            let pixels = pixmap.data_as_u8_slice();
            let non_opaque = pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[3] != 255)
                .count();
            assert_eq!(
                non_opaque,
                0,
                "page {} has non-opaque pixels (min alpha {})",
                page + 1,
                pixels.chunks_exact(4).map(|pixel| pixel[3]).min().unwrap()
            );
            let ink = pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[..3].iter().any(|&channel| channel < 200))
                .count();
            assert!(
                ink > 1_000,
                "page {} rendered without its content",
                page + 1
            );
            if let Ok(directory) = std::env::var("FLIGHT_PDF_TEST_OUTPUT") {
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(
                    format!("{directory}/problem-{}.png", page + 1),
                    pixmap.into_png().unwrap(),
                )
                .unwrap();
            }
        }
        assert!(
            render_page(
                &pdf,
                &cache,
                RenderKey {
                    page: 9,
                    width: 612
                }
            )
            .is_err()
        );
        assert!(
            render_page(
                &pdf,
                &cache,
                RenderKey {
                    page: 0,
                    width: 20_000
                }
            )
            .is_err()
        );
    }
}
