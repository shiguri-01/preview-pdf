use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::{AppState, NoticeAction, PaletteRequest};
use crate::backend::{OutlineNode, PdfBackend, SharedPdfBackend};
use crate::command::CommandOutcome;
use crate::error::{AppError, AppResult};
use crate::palette::PaletteKind;

use super::palette::OutlinePaletteEntry;

struct OutlineCache {
    doc_id: u64,
    entries: Arc<[OutlinePaletteEntry]>,
}

#[derive(Default)]
pub struct OutlineState {
    cache: Option<OutlineCache>,
}

impl OutlineState {
    pub fn open_palette(
        &mut self,
        pdf: SharedPdfBackend,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.ensure_loaded(pdf.as_ref())?;
        palette_requests.push_back(PaletteRequest::Open {
            kind: PaletteKind::Outline,
            seed: None,
        });
        Ok((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub fn goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        if page >= page_count {
            return Err(AppError::page_out_of_range(
                page.saturating_add(1),
                page_count,
            ));
        }

        let target = app.normalize_page_for_layout(page, page_count);
        app.current_page = target;
        Ok((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub fn palette_entries(&self) -> Arc<[OutlinePaletteEntry]> {
        self.cache
            .as_ref()
            .map(|cache| Arc::clone(&cache.entries))
            .unwrap_or_else(|| Arc::from([]))
    }

    fn ensure_loaded(&mut self, pdf: &dyn PdfBackend) -> AppResult<()> {
        if self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.doc_id == pdf.doc_id())
        {
            return Ok(());
        }

        let outline = pdf.extract_outline()?;
        let mut entries = Vec::new();
        flatten_outline(&outline, 0, &mut entries);
        self.cache = Some(OutlineCache {
            doc_id: pdf.doc_id(),
            entries: Arc::from(entries),
        });
        Ok(())
    }
}

fn flatten_outline(nodes: &[OutlineNode], depth: usize, entries: &mut Vec<OutlinePaletteEntry>) {
    let mut stack = nodes
        .iter()
        .rev()
        .map(|node| (node, depth))
        .collect::<Vec<_>>();

    while let Some((node, node_depth)) = stack.pop() {
        entries.push(OutlinePaletteEntry {
            title: node.title.clone(),
            page: node.page,
            depth: node_depth,
        });

        stack.extend(
            node.children
                .iter()
                .rev()
                .map(|child| (child, node_depth + 1)),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::backend::{OutlineNode, PdfBackend, RgbaFrame, SharedPdfBackend, TextPage};

    use super::OutlineState;

    struct StubPdf {
        path: PathBuf,
        doc_id: u64,
        outline: Vec<OutlineNode>,
    }

    impl PdfBackend for StubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            self.doc_id
        }

        fn page_count(&self) -> usize {
            4
        }

        fn page_dimensions(&self, _page: usize) -> crate::error::AppResult<(f32, f32)> {
            Ok((1.0, 1.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
            Ok(RgbaFrame {
                width: 1,
                height: 1,
                pixels: vec![0, 0, 0, 0].into(),
            })
        }

        fn extract_text_page(&self, _page: usize) -> crate::error::AppResult<TextPage> {
            Ok(TextPage {
                width_pt: 1.0,
                height_pt: 1.0,
                glyphs: Vec::new(),
            })
        }

        fn extract_outline(&self) -> crate::error::AppResult<Vec<OutlineNode>> {
            Ok(self.outline.clone())
        }
    }

    #[test]
    fn palette_entries_flatten_outline_depth_first() {
        let pdf = Arc::new(StubPdf {
            path: PathBuf::from("outline.pdf"),
            doc_id: 9,
            outline: vec![OutlineNode {
                title: "Root".to_string(),
                page: 0,
                children: vec![OutlineNode {
                    title: "Child".to_string(),
                    page: 2,
                    children: Vec::new(),
                }],
            }],
        }) as SharedPdfBackend;
        let mut state = OutlineState::default();
        let mut pending = VecDeque::new();

        state
            .open_palette(pdf, &mut pending)
            .expect("outline open should succeed");
        let entries = state.palette_entries();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Root");
        assert_eq!(entries[0].depth, 0);
        assert_eq!(entries[1].title, "Child");
        assert_eq!(entries[1].depth, 1);
    }

    #[test]
    fn palette_entries_preserve_depth_first_sibling_order() {
        let pdf = Arc::new(StubPdf {
            path: PathBuf::from("outline.pdf"),
            doc_id: 8,
            outline: vec![
                OutlineNode {
                    title: "Root".to_string(),
                    page: 0,
                    children: vec![
                        OutlineNode {
                            title: "Child A".to_string(),
                            page: 1,
                            children: Vec::new(),
                        },
                        OutlineNode {
                            title: "Child B".to_string(),
                            page: 2,
                            children: Vec::new(),
                        },
                    ],
                },
                OutlineNode {
                    title: "Second Root".to_string(),
                    page: 3,
                    children: Vec::new(),
                },
            ],
        }) as SharedPdfBackend;
        let mut state = OutlineState::default();
        let mut pending = VecDeque::new();

        state
            .open_palette(pdf, &mut pending)
            .expect("outline open should succeed");
        let entries = state.palette_entries();

        let titles = entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["Root", "Child A", "Child B", "Second Root"]);
        assert_eq!(entries[3].depth, 0);
    }

    #[test]
    fn palette_entries_reuse_cached_arc_for_same_document() {
        let pdf = Arc::new(StubPdf {
            path: PathBuf::from("outline.pdf"),
            doc_id: 9,
            outline: vec![OutlineNode {
                title: "Root".to_string(),
                page: 0,
                children: Vec::new(),
            }],
        }) as SharedPdfBackend;
        let mut state = OutlineState::default();
        let mut pending = VecDeque::new();

        state
            .open_palette(Arc::clone(&pdf), &mut pending)
            .expect("first outline open should succeed");
        let first = state.palette_entries();

        state
            .open_palette(pdf, &mut pending)
            .expect("second outline open should succeed");
        let second = state.palette_entries();

        assert!(Arc::ptr_eq(&first, &second));
    }
}
