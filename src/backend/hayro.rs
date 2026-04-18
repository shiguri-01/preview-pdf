use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytemuck::allocation::cast_vec;
use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::util::{PageExt, RectExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterSettings, Paint,
    PathDrawMode, SoftMask, interpret_page,
};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{
    A, D, DEST, DESTS, FIRST, KIDS, NAMES, NEXT, OUTLINES, S, TITLE,
};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, ObjRef, Object, ObjectIdentifier};
use hayro::hayro_syntax::page::Page;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, color::PremulRgba8};
use hayro::{RenderSettings, render};
use kurbo::{Affine, BezPath, Point, Shape};

use crate::error::{AppError, AppResult};

use super::traits::{OutlineNode, PdfBackend, PdfRect, RgbaFrame, TextGlyph, TextPage};

pub struct PdfDoc {
    path: PathBuf,
    doc_id: u64,
    pdf: Pdf,
}

pub type HayroPdfBackend = PdfDoc;

impl PdfBackend for PdfDoc {
    fn path(&self) -> &Path {
        PdfDoc::path(self)
    }

    fn doc_id(&self) -> u64 {
        PdfDoc::doc_id(self)
    }

    fn page_count(&self) -> usize {
        PdfDoc::page_count(self)
    }

    fn page_dimensions(&self, page: usize) -> AppResult<(f32, f32)> {
        PdfDoc::page_render_dimensions(self, page)
    }

    fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame> {
        PdfDoc::render_page(self, page, scale)
    }

    fn extract_text(&self, page: usize) -> AppResult<String> {
        PdfDoc::extract_text(self, page)
    }

    fn extract_positioned_text(&self, page: usize) -> AppResult<TextPage> {
        PdfDoc::extract_positioned_text(self, page)
    }

    fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
        PdfDoc::extract_outline(self)
    }
}

impl PdfDoc {
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        let bytes = Self::load_shared_bytes(path)?;
        Self::open_with_shared_bytes(path, bytes)
    }

    pub fn load_shared_bytes(path: impl AsRef<Path>) -> AppResult<Arc<Vec<u8>>> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(AppError::invalid_argument("pdf path must not be empty"));
        }
        if !path.exists() {
            return Err(AppError::io_with_context(
                std::io::Error::new(std::io::ErrorKind::NotFound, "missing file"),
                format!("pdf file not found: {}", path.display()),
            ));
        }
        if !path.is_file() {
            return Err(AppError::invalid_argument(
                "pdf path must be a regular file",
            ));
        }

        let bytes = Arc::new(std::fs::read(path)?);
        if !bytes.as_slice().starts_with(b"%PDF-") {
            return Err(AppError::invalid_argument(
                "input is not a valid PDF header",
            ));
        }

        Ok(bytes)
    }

    pub fn open_with_shared_bytes(path: impl AsRef<Path>, bytes: Arc<Vec<u8>>) -> AppResult<Self> {
        let path = path.as_ref();
        if !bytes.as_slice().starts_with(b"%PDF-") {
            return Err(AppError::invalid_argument(
                "input is not a valid PDF header",
            ));
        }
        let doc_id = calculate_doc_id(path, bytes.len());
        let pdf = Pdf::new(bytes)
            .map_err(|_| AppError::invalid_argument("failed to parse PDF with hayro"))?;

        Ok(Self {
            path: path.to_path_buf(),
            doc_id,
            pdf,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn doc_id(&self) -> u64 {
        self.doc_id
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    pub fn page_render_dimensions(&self, page: usize) -> AppResult<(f32, f32)> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(page_ref.render_dimensions())
    }

    pub fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(AppError::invalid_argument(
                "scale must be a positive finite value",
            ));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        let render_settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        };
        let interpreter_settings = InterpreterSettings::default();
        let pixmap = render(page_ref, &interpreter_settings, &render_settings);

        Ok(RgbaFrame {
            width: pixmap.width() as u32,
            height: pixmap.height() as u32,
            pixels: pixel_buffer_from_pixmap(pixmap).into(),
        })
    }

    pub fn extract_text(&self, page: usize) -> AppResult<String> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(extract_text_with_device(page_ref).trim().to_owned())
    }

    pub fn extract_positioned_text(&self, page: usize) -> AppResult<TextPage> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(extract_positioned_text_with_device(page_ref))
    }

    pub fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
        extract_outline_nodes(&self.pdf)
    }
}

fn extract_outline_nodes(pdf: &Pdf) -> AppResult<Vec<OutlineNode>> {
    let xref = pdf.xref();
    let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else {
        return Err(AppError::unsupported("failed to resolve pdf catalog"));
    };
    let Some(outlines_ref) = root.get_ref(OUTLINES) else {
        return Ok(Vec::new());
    };
    let Some(outlines) = xref.get::<Dict<'_>>(outlines_ref.into()) else {
        return Ok(Vec::new());
    };
    let Some(first_ref) = outlines.get_ref(FIRST) else {
        return Ok(Vec::new());
    };

    let page_index = pdf
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(page, pdf_page)| pdf_page.raw().obj_id().map(|id| (id, page)))
        .collect::<HashMap<_, _>>();
    let named_destinations = build_named_destination_index(root);
    let mut visited = HashSet::new();

    Ok(read_outline_siblings(
        xref,
        first_ref.into(),
        &page_index,
        &named_destinations,
        &mut visited,
    ))
}

fn read_outline_siblings<'a>(
    xref: &'a hayro::hayro_syntax::xref::XRef,
    start: ObjectIdentifier,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();
    let mut current = Some(start);

    while let Some(id) = current {
        if !visited.insert(id) {
            break;
        }

        let Some(item) = xref.get::<Dict<'_>>(id) else {
            break;
        };
        let next = item.get_ref(NEXT).map(Into::into);
        let mut children = item
            .get_ref(FIRST)
            .map(|first| {
                read_outline_siblings(xref, first.into(), page_index, named_destinations, visited)
            })
            .unwrap_or_default();

        if let Some(page) = resolve_outline_page(&item, xref, page_index, named_destinations) {
            nodes.push(OutlineNode {
                title: outline_title(&item),
                page,
                children,
            });
        } else {
            nodes.append(&mut children);
        }

        current = next;
    }

    nodes
}

fn resolve_outline_page<'a>(
    item: &Dict<'a>,
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
) -> Option<usize> {
    if let Some(dest) = item.get_raw::<Object<'_>>(DEST) {
        return resolve_destination(
            dest,
            xref,
            page_index,
            named_destinations,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
    }

    let action = item.get::<Dict<'_>>(A)?;
    let action_kind = action.get::<Name<'_>>(S)?;
    if action_kind.as_str() != "GoTo" {
        return None;
    }

    let dest = action.get_raw::<Object<'_>>(D)?;
    resolve_destination(
        dest,
        xref,
        page_index,
        named_destinations,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}

fn resolve_destination<'a>(
    value: MaybeRef<Object<'a>>,
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
    visited_names: &mut HashSet<Vec<u8>>,
) -> Option<usize> {
    let object = match value {
        MaybeRef::Ref(obj_ref) => {
            let id: ObjectIdentifier = obj_ref.into();
            if !visited.insert(id) {
                return None;
            }
            xref.get::<Object<'_>>(id)?
        }
        MaybeRef::NotRef(object) => object,
    };

    match object {
        Object::Array(array) => resolve_destination_array(&array, page_index),
        Object::Dict(dict) => dict.get_raw::<Object<'_>>(D).and_then(|dest| {
            resolve_destination(
                dest,
                xref,
                page_index,
                named_destinations,
                visited,
                visited_names,
            )
        }),
        Object::Name(name) => resolve_named_destination(
            name.as_ref(),
            xref,
            page_index,
            named_destinations,
            visited,
            visited_names,
        ),
        Object::String(string) => resolve_named_destination(
            string.get().as_ref(),
            xref,
            page_index,
            named_destinations,
            visited,
            visited_names,
        ),
        Object::Null(_) | Object::Boolean(_) | Object::Number(_) | Object::Stream(_) => None,
    }
}

fn resolve_destination_array(
    array: &Array<'_>,
    page_index: &HashMap<ObjectIdentifier, usize>,
) -> Option<usize> {
    match array.raw_iter().next()? {
        MaybeRef::Ref(page_ref) => page_index.get(&page_ref.into()).copied(),
        MaybeRef::NotRef(Object::Dict(page_dict)) => page_dict
            .obj_id()
            .and_then(|id| page_index.get(&id).copied()),
        MaybeRef::NotRef(_) => None,
    }
}

fn outline_title(item: &Dict<'_>) -> String {
    let Some(title) = item.get::<hayro::hayro_syntax::object::String<'_>>(TITLE) else {
        return "(untitled)".to_string();
    };

    let decoded = decode_pdf_text_string(title.get().as_ref())
        .trim()
        .to_string();
    if decoded.is_empty() {
        "(untitled)".to_string()
    } else {
        decoded
    }
}

fn decode_pdf_text_string(bytes: &[u8]) -> String {
    if let Some(decoded) = decode_bom_prefixed_text(bytes) {
        return decoded;
    }

    bytes
        .iter()
        .map(|byte| decode_pdf_doc_encoding_byte(*byte))
        .collect()
}

fn decode_bom_prefixed_text(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => decode_utf16_bytes(rest, Utf16Endian::Big),
        [0xFF, 0xFE, rest @ ..] => decode_utf16_bytes(rest, Utf16Endian::Little),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Some(String::from_utf8_lossy(rest).into_owned()),
        _ => None,
    }
}

fn decode_utf16_bytes(bytes: &[u8], endian: Utf16Endian) -> Option<String> {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }

    let code_units = chunks
        .map(|chunk| match endian {
            Utf16Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
            Utf16Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
        })
        .collect::<Vec<_>>();

    Some(String::from_utf16_lossy(&code_units))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Utf16Endian {
    Big,
    Little,
}

fn build_named_destination_index<'a>(root: Dict<'a>) -> HashMap<Vec<u8>, NamedDestination<'a>> {
    let mut destinations = HashMap::new();
    let mut visited = HashSet::new();

    if let Some(dests) = root.get::<Dict<'_>>(DESTS) {
        collect_destination_dict(dests, &mut destinations);
    }

    if let Some(names_root) = root.get::<Dict<'_>>(NAMES)
        && let Some(dests_tree) = names_root.get::<Dict<'_>>(DESTS)
    {
        collect_name_tree_destinations(dests_tree, &mut destinations, &mut visited);
    }
    destinations
}

fn collect_destination_dict<'a>(
    dict: Dict<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
) {
    for (key, value) in dict.entries() {
        destinations.insert(
            key.as_ref().to_vec(),
            NamedDestination::from_maybe_ref(value),
        );
    }
}

fn collect_name_tree_destinations<'a>(
    dict: Dict<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
) {
    if let Some(id) = dict.obj_id()
        && !visited.insert(id)
    {
        return;
    }

    if let Some(names) = dict.get::<Array<'_>>(NAMES) {
        collect_name_tree_pairs(names, destinations);
    }

    if let Some(kids) = dict.get::<Array<'_>>(KIDS) {
        for child in kids.iter::<Dict<'_>>() {
            collect_name_tree_destinations(child, destinations, visited);
        }
    }
}

fn collect_name_tree_pairs<'a>(
    array: Array<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
) {
    let mut iter = array.raw_iter();
    while let Some(name) = iter.next() {
        let Some(dest) = iter.next() else {
            break;
        };

        if let Some(key) = destination_name_key(name) {
            destinations.insert(key, NamedDestination::from_maybe_ref(dest));
        }
    }
}

fn destination_name_key(value: MaybeRef<Object<'_>>) -> Option<Vec<u8>> {
    match value {
        MaybeRef::Ref(_) => None,
        MaybeRef::NotRef(Object::Name(name)) => Some(name.as_ref().to_vec()),
        MaybeRef::NotRef(Object::String(string)) => Some(string.get().into_owned()),
        MaybeRef::NotRef(_) => None,
    }
}

fn resolve_named_destination<'a>(
    name: &[u8],
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
    visited_names: &mut HashSet<Vec<u8>>,
) -> Option<usize> {
    if !visited_names.insert(name.to_vec()) {
        return None;
    }
    let dest = named_destinations.get(name)?.to_maybe_ref();
    resolve_destination(
        dest,
        xref,
        page_index,
        named_destinations,
        visited,
        visited_names,
    )
}

fn decode_pdf_doc_encoding_byte(byte: u8) -> char {
    match byte {
        0x16 => '\u{0016}',
        0x18 => '\u{02D8}',
        0x19 => '\u{02C7}',
        0x1A => '\u{02C6}',
        0x1B => '\u{02D9}',
        0x1C => '\u{02DD}',
        0x1D => '\u{02DB}',
        0x1E => '\u{02DA}',
        0x1F => '\u{02DC}',
        0x7F => '\u{FFFD}',
        0x80 => '\u{2022}',
        0x81 => '\u{2020}',
        0x82 => '\u{2021}',
        0x83 => '\u{2026}',
        0x84 => '\u{2014}',
        0x85 => '\u{2013}',
        0x86 => '\u{0192}',
        0x87 => '\u{2044}',
        0x88 => '\u{2039}',
        0x89 => '\u{203A}',
        0x8A => '\u{2212}',
        0x8B => '\u{2030}',
        0x8C => '\u{201E}',
        0x8D => '\u{201C}',
        0x8E => '\u{201D}',
        0x8F => '\u{2018}',
        0x90 => '\u{2019}',
        0x91 => '\u{201A}',
        0x92 => '\u{2122}',
        0x93 => '\u{FB01}',
        0x94 => '\u{FB02}',
        0x95 => '\u{0141}',
        0x96 => '\u{0152}',
        0x97 => '\u{0160}',
        0x98 => '\u{0178}',
        0x99 => '\u{017D}',
        0x9A => '\u{0131}',
        0x9B => '\u{0142}',
        0x9C => '\u{0153}',
        0x9D => '\u{0161}',
        0x9E => '\u{017E}',
        _ => byte as char,
    }
}

#[derive(Debug, Clone)]
enum NamedDestination<'a> {
    Direct(Object<'a>),
    Ref(ObjRef),
}

impl<'a> NamedDestination<'a> {
    fn from_maybe_ref(value: MaybeRef<Object<'a>>) -> Self {
        match value {
            MaybeRef::Ref(obj_ref) => Self::Ref(obj_ref),
            MaybeRef::NotRef(object) => Self::Direct(object),
        }
    }

    fn to_maybe_ref(&self) -> MaybeRef<Object<'a>> {
        match self {
            Self::Direct(object) => MaybeRef::NotRef(object.clone()),
            Self::Ref(obj_ref) => MaybeRef::Ref(*obj_ref),
        }
    }
}

fn extract_text_with_device(page: &Page<'_>) -> String {
    let mut context = Context::new(
        page.initial_transform(true),
        page.intersected_crop_box().to_kurbo(),
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = PlainTextExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish()
}

#[derive(Default)]
struct PlainTextExtractDevice {
    text: String,
    last_point: Option<Point>,
    last_glyph: Option<(char, i32, i32)>,
}

impl PlainTextExtractDevice {
    fn finish(self) -> String {
        self.text
    }

    fn push_char(&mut self, ch: char, x: f64, y: f64) {
        if ch == '\n' || ch == '\r' {
            push_plain_newline(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }
        if ch.is_whitespace() {
            push_plain_space(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }

        if let Some(last) = self.last_point
            && (y - last.y).abs() > PLAIN_TEXT_LINE_BREAK_THRESHOLD
        {
            push_plain_newline(&mut self.text);
        }

        self.text.push(ch);
        self.last_point = Some(Point::new(x, y));
    }

    fn is_duplicate_glyph(&self, ch: char, x: f64, y: f64) -> bool {
        self.last_glyph == Some((ch, quantize_coord(x), quantize_coord(y)))
    }

    fn set_last_glyph(&mut self, ch: char, x: f64, y: f64) {
        self.last_glyph = Some((ch, quantize_coord(x), quantize_coord(y)));
    }
}

impl<'a> Device<'a> for PlainTextExtractDevice {
    fn set_soft_mask(&mut self, _mask: Option<SoftMask<'a>>) {}

    fn set_blend_mode(&mut self, _blend_mode: BlendMode) {}

    fn draw_path(
        &mut self,
        _path: &BezPath,
        _transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &PathDrawMode,
    ) {
    }

    fn push_clip_path(&mut self, _clip_path: &ClipPath) {}

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<SoftMask<'a>>,
        _blend_mode: BlendMode,
    ) {
    }

    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &GlyphDrawMode,
    ) {
        let Some(ch) = glyph.as_unicode() else {
            return;
        };

        let position = (transform * glyph_transform) * Point::ORIGIN;
        if self.is_duplicate_glyph(ch, position.x, position.y) {
            return;
        }

        self.set_last_glyph(ch, position.x, position.y);
        self.push_char(ch, position.x, position.y);
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

fn extract_positioned_text_with_device(page: &Page<'_>) -> TextPage {
    let mut context = Context::new(
        page.initial_transform(true),
        page.intersected_crop_box().to_kurbo(),
        page.xref(),
        InterpreterSettings::default(),
    );
    let (width_pt, height_pt) = page.render_dimensions();
    let mut device = PositionedTextExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish(width_pt, height_pt)
}

#[derive(Default)]
struct PositionedTextExtractDevice {
    last_glyph: Option<(char, i32, i32)>,
    glyphs: Vec<TextGlyph>,
    dropped_glyphs: usize,
}

impl PositionedTextExtractDevice {
    fn finish(self, width_pt: f32, height_pt: f32) -> TextPage {
        TextPage {
            width_pt,
            height_pt,
            glyphs: self.glyphs,
            dropped_glyphs: self.dropped_glyphs,
        }
    }

    fn is_duplicate_glyph(&self, ch: char, x: f64, y: f64) -> bool {
        self.last_glyph == Some((ch, quantize_coord(x), quantize_coord(y)))
    }

    fn set_last_glyph(&mut self, ch: char, x: f64, y: f64) {
        self.last_glyph = Some((ch, quantize_coord(x), quantize_coord(y)));
    }
}

impl<'a> Device<'a> for PositionedTextExtractDevice {
    fn set_soft_mask(&mut self, _mask: Option<SoftMask<'a>>) {}

    fn set_blend_mode(&mut self, _blend_mode: BlendMode) {}

    fn draw_path(
        &mut self,
        _path: &BezPath,
        _transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &PathDrawMode,
    ) {
    }

    fn push_clip_path(&mut self, _clip_path: &ClipPath) {}

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<SoftMask<'a>>,
        _blend_mode: BlendMode,
    ) {
    }

    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &GlyphDrawMode,
    ) {
        let Some(ch) = glyph.as_unicode() else {
            return;
        };

        let position = (transform * glyph_transform) * Point::ORIGIN;
        if self.is_duplicate_glyph(ch, position.x, position.y) {
            return;
        }

        self.set_last_glyph(ch, position.x, position.y);
        if let Some(bbox) = glyph_bbox(glyph, transform, glyph_transform) {
            self.glyphs.push(TextGlyph { ch, bbox });
        } else {
            self.dropped_glyphs += 1;
        }
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

fn quantize_coord(value: f64) -> i32 {
    (value * 100.0).round() as i32
}

const PLAIN_TEXT_LINE_BREAK_THRESHOLD: f64 = 6.0;

fn push_plain_newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_plain_space(out: &mut String) {
    if !out.ends_with([' ', '\n']) {
        out.push(' ');
    }
}

fn glyph_bbox(glyph: &Glyph<'_>, transform: Affine, glyph_transform: Affine) -> Option<PdfRect> {
    let outline = match glyph {
        Glyph::Outline(outline) => outline.outline(),
        Glyph::Type3(_) => return None,
    };
    let bbox = (transform * glyph_transform * outline).bounding_box();
    if bbox.is_zero_area() {
        return None;
    }

    Some(PdfRect {
        x0: bbox.x0 as f32,
        y0: bbox.y0 as f32,
        x1: bbox.x1 as f32,
        y1: bbox.y1 as f32,
    })
}

fn calculate_doc_id(path: &Path, byte_len: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    byte_len.hash(&mut hasher);
    hasher.finish()
}

fn pixel_buffer_from_pixmap(pixmap: Pixmap) -> Vec<u8> {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let pixels: Vec<PremulRgba8> = pixmap.take();
    let bytes = cast_vec(pixels);
    debug_assert_eq!(bytes.len(), width * height * 4);
    bytes
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use hayro::vello_cpu::Pixmap;

    use crate::error::AppError;

    use super::{PdfDoc, decode_pdf_text_string, pixel_buffer_from_pixmap};

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        let mut path = std::env::temp_dir();
        path.push(format!("pvf_{suffix}_{}_{}", process::id(), nanos));
        path
    }

    #[test]
    fn open_rejects_directory_path() {
        let dir = unique_temp_path("dir");
        fs::create_dir_all(&dir).expect("test directory should be created");

        let result = PdfDoc::open(&dir);
        assert!(matches!(
            result,
            Err(AppError::InvalidArgument(message))
                if message == "pdf path must be a regular file"
        ));

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[test]
    fn open_accepts_valid_pdf_with_page_count() {
        let file = unique_temp_path("file.pdf");
        fs::write(&file, build_pdf(&["first page", "second page"]))
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("regular file path should be accepted");
        assert_eq!(doc.path(), file.as_path());
        assert_eq!(doc.page_count(), 2);
        assert_ne!(doc.doc_id(), 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn render_page_rejects_out_of_range_page() {
        let file = unique_temp_path("render.pdf");
        fs::write(&file, build_pdf(&["hello"])).expect("test file should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");

        let err = doc.render_page(8, 1.0).expect_err("page should be invalid");
        assert!(matches!(
            err,
            AppError::InvalidArgument(message) if message == "page index is out of range"
        ));

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn page_render_dimensions_read_page_size() {
        let file = unique_temp_path("dimensions.pdf");
        fs::write(&file, build_pdf(&["hello"])).expect("test file should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");

        let (width, height) = doc
            .page_render_dimensions(0)
            .expect("dimensions should be available");
        assert!((width - 300.0).abs() < f32::EPSILON);
        assert!((height - 300.0).abs() < f32::EPSILON);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_text_returns_page_bucket_text() {
        let file = unique_temp_path("text.pdf");
        fs::write(&file, build_pdf(&["hello world", "second page"]))
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let text = doc.extract_text(0).expect("extract should succeed");
        let normalized: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(normalized.contains("helloworld"));

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_text_does_not_insert_false_space_from_tj_position_gap() {
        let file = unique_temp_path("tj_gap.pdf");
        fs::write(
            &file,
            build_pdf_with_raw_streams(&["BT /F1 14 Tf 36 260 Td [(hello) -220 (world)] TJ ET"]),
        )
        .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let text = doc.extract_text(0).expect("extract should succeed");
        let normalized: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            normalized.to_lowercase().contains("helloworld"),
            "expected stable extraction without false splits, got: {text:?}"
        );

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn render_page_uses_hayro_pixmap_output() {
        let file = unique_temp_path("pixmap.pdf");
        fs::write(&file, build_pdf(&["render me"])).expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let frame = doc.render_page(0, 1.0).expect("render should succeed");
        assert!(frame.width > 0);
        assert!(frame.height > 0);
        assert_eq!(
            frame.pixels.len(),
            frame.width as usize * frame.height as usize * 4
        );

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn pixel_buffer_from_pixmap_matches_slice_copy_bytes() {
        let mut pixmap = Pixmap::new(2, 1);
        let expected = {
            let bytes = pixmap.data_as_u8_slice_mut();
            bytes.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
            pixmap.data_as_u8_slice().to_vec()
        };

        assert_eq!(pixel_buffer_from_pixmap(pixmap), expected);
    }

    #[test]
    fn decode_pdf_text_string_decodes_utf16be_bom() {
        let decoded =
            decode_pdf_text_string(&[0xFE, 0xFF, 0x30, 0x42, 0x30, 0x44, 0x30, 0x46, 0x30, 0x48]);
        assert_eq!(decoded, "あいうえ");
    }

    #[test]
    fn decode_pdf_text_string_falls_back_to_utf8_lossy_without_bom() {
        let decoded = decode_pdf_text_string("outline".as_bytes());
        assert_eq!(decoded, "outline");
    }

    #[test]
    fn decode_pdf_text_string_uses_pdfdoc_encoding_without_bom() {
        let decoded = decode_pdf_text_string(&[0x8D, b'A', 0x8E]);
        assert_eq!(decoded, "\u{201C}A\u{201D}");
    }

    #[test]
    fn decode_pdf_text_string_decodes_pdfdoc_encoding_control_byte_0x16() {
        let decoded = decode_pdf_text_string(&[0x16]);
        assert_eq!(decoded, "\u{0016}");
    }

    #[test]
    fn extract_outline_resolves_named_destinations_from_name_tree() {
        let file = unique_temp_path("outline_named_dest.pdf");
        fs::write(&file, build_pdf_with_named_outline()).expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].title, "Chapter 1");
        assert_eq!(outline[0].page, 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_outline_handles_cyclic_named_destination_tree() {
        let file = unique_temp_path("outline_named_dest_cycle.pdf");
        fs::write(&file, build_pdf_with_cyclic_named_outline())
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].title, "Chapter 1");
        assert_eq!(outline[0].page, 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_outline_handles_named_destination_alias_cycles() {
        let file = unique_temp_path("outline_named_dest_alias_cycle.pdf");
        fs::write(&file, build_pdf_with_cyclic_named_outline_aliases())
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert!(outline.is_empty());

        fs::remove_file(&file).expect("test file should be removed");
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_texts = if page_texts.is_empty() {
            vec!["".to_string()]
        } else {
            page_texts
                .iter()
                .map(|text| {
                    let escaped = escape_literal_string(text);
                    format!("BT /F1 14 Tf 36 260 Td ({escaped}) Tj ET")
                })
                .collect()
        };

        build_pdf_from_streams(&page_texts)
    }

    fn build_pdf_with_raw_streams(page_streams: &[&str]) -> Vec<u8> {
        let page_streams = if page_streams.is_empty() {
            vec!["".to_string()]
        } else {
            page_streams
                .iter()
                .map(|stream| (*stream).to_string())
                .collect()
        };

        build_pdf_from_streams(&page_streams)
    }

    fn build_pdf_from_streams(page_streams: &[String]) -> Vec<u8> {
        let page_count = page_streams.len();
        let page_ids: Vec<usize> = (0..page_count).map(|i| 4 + i * 2).collect();

        let mut objects = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());

        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

        for (index, stream) in page_streams.iter().enumerate() {
            let content_id = 5 + index * 2;

            let page_obj = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
            );
            let content_obj = format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            );

            objects.push(page_obj);
            objects.push(content_obj);
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

        let mut offsets = Vec::new();
        offsets.push(0_usize);
        for (index, object) in objects.iter().enumerate() {
            let object_id = index + 1;
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
        }

        let xref_start = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_start
            )
            .as_bytes(),
        );

        bytes
    }

    fn build_pdf_with_named_outline() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 8 0 R >>".to_string(),
            "<< /Title (Chapter 1) /Parent 4 0 R /Dest (chapter-1) >>".to_string(),
            "<< /Names [(chapter-1) [5 0 R /Fit]] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }

    fn build_pdf_with_cyclic_named_outline() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 9 0 R >>".to_string(),
            "<< /Title (Chapter 1) /Parent 4 0 R /Dest (chapter-1) >>".to_string(),
            "<< /Kids [8 0 R] >>".to_string(),
            "<< /Names [(chapter-1) [5 0 R /Fit]] /Kids [7 0 R] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }

    fn build_pdf_with_cyclic_named_outline_aliases() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 8 0 R >>".to_string(),
            "<< /Title (Loop) /Parent 4 0 R /Dest (A) >>".to_string(),
            "<< /Names [(A) (B) (B) (A)] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }

    fn build_pdf_from_objects(objects: &[String]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

        let mut offsets = Vec::new();
        offsets.push(0_usize);
        for (index, object) in objects.iter().enumerate() {
            let object_id = index + 1;
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
        }

        let xref_start = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_start
            )
            .as_bytes(),
        );

        bytes
    }

    fn escape_literal_string(text: &str) -> String {
        let mut out = String::with_capacity(text.len());

        for ch in text.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '(' => out.push_str("\\("),
                ')' => out.push_str("\\)"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }

        out
    }
}
