use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::backend::PdfRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightSource {
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HighlightStyle {
    pub fill_rgba: [u8; 4],
    pub priority: u8,
}

impl HighlightStyle {
    pub const SEARCH_HIT: Self = Self {
        fill_rgba: [255, 196, 79, 96],
        priority: 0,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct HighlightSpan {
    pub source: HighlightSource,
    pub page: usize,
    pub rects: Vec<PdfRect>,
    pub style: HighlightStyle,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HighlightOverlaySnapshot {
    pub spans: Vec<HighlightSpan>,
    pub stamp: u64,
}

impl HighlightOverlaySnapshot {
    pub fn new(spans: Vec<HighlightSpan>) -> Self {
        let mut snapshot = Self { spans, stamp: 0 };
        snapshot.stamp = snapshot.compute_stamp();
        snapshot
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    fn compute_stamp(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for span in &self.spans {
            span.source.hash(&mut hasher);
            span.page.hash(&mut hasher);
            span.style.hash(&mut hasher);
            for rect in &span.rects {
                quantize(rect.x0).hash(&mut hasher);
                quantize(rect.y0).hash(&mut hasher);
                quantize(rect.x1).hash(&mut hasher);
                quantize(rect.y1).hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

fn quantize(value: f32) -> i32 {
    (value * 100.0).round() as i32
}
