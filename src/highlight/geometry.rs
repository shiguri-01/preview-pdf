use crate::backend::{PdfRect, TextGlyph};

pub fn merge_text_glyph_rects(glyphs: &[TextGlyph]) -> Vec<PdfRect> {
    let glyphs: Vec<HighlightGlyph> = glyphs
        .iter()
        .filter_map(|glyph| {
            if glyph.ch.is_whitespace() {
                None
            } else {
                glyph.bbox.map(|bbox| HighlightGlyph { bbox })
            }
        })
        .collect();
    if glyphs.is_empty() {
        return Vec::new();
    }

    let merge_axis = infer_merge_axis(&glyphs);
    let median_width = median_rect_extent(&glyphs, RectExtent::Width);
    let median_height = median_rect_extent(&glyphs, RectExtent::Height);
    let mut rects = Vec::new();
    let mut current = glyphs[0].bbox;

    for glyph in glyphs.iter().skip(1) {
        let bbox = glyph.bbox;
        if belongs_to_run(current, bbox, merge_axis, median_width, median_height) {
            current = union_rects(current, bbox);
        } else {
            rects.push(current);
            current = bbox;
        }
    }

    rects.push(current);
    rects
}

#[derive(Debug, Clone, Copy)]
struct HighlightGlyph {
    bbox: PdfRect,
}

fn infer_merge_axis(glyphs: &[HighlightGlyph]) -> MergeAxis {
    let mut horizontal_score = 0.0f32;
    let mut vertical_score = 0.0f32;

    for pair in glyphs.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        let left_bbox = left.bbox;
        let right_bbox = right.bbox;
        horizontal_score +=
            overlap_ratio_1d(left_bbox.y0, left_bbox.y1, right_bbox.y0, right_bbox.y1);
        vertical_score +=
            overlap_ratio_1d(left_bbox.x0, left_bbox.x1, right_bbox.x0, right_bbox.x1);
    }

    if vertical_score > horizontal_score {
        MergeAxis::Vertical
    } else {
        MergeAxis::Horizontal
    }
}

fn belongs_to_run(
    current: PdfRect,
    next: PdfRect,
    merge_axis: MergeAxis,
    median_width: f32,
    median_height: f32,
) -> bool {
    match merge_axis {
        MergeAxis::Horizontal => {
            let same_band = overlap_ratio_1d(current.y0, current.y1, next.y0, next.y1) >= 0.45
                || center_distance(current.y0, current.y1, next.y0, next.y1)
                    <= median_height * 0.35;
            let gap_ok =
                interval_gap(current.x0, current.x1, next.x0, next.x1) <= median_width * 4.0;
            same_band && gap_ok
        }
        MergeAxis::Vertical => {
            let same_band = overlap_ratio_1d(current.x0, current.x1, next.x0, next.x1) >= 0.45
                || center_distance(current.x0, current.x1, next.x0, next.x1) <= median_width * 0.35;
            let gap_ok =
                interval_gap(current.y0, current.y1, next.y0, next.y1) <= median_height * 4.0;
            same_band && gap_ok
        }
    }
}

fn overlap_ratio_1d(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    let overlap = (a1.min(b1) - a0.max(b0)).max(0.0);
    let min_extent = (a1 - a0).abs().min((b1 - b0).abs()).max(1e-3);
    overlap / min_extent
}

fn center_distance(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (((a0 + a1) * 0.5) - ((b0 + b1) * 0.5)).abs()
}

fn interval_gap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    if b0 > a1 {
        b0 - a1
    } else if a0 > b1 {
        a0 - b1
    } else {
        0.0
    }
}

fn union_rects(left: PdfRect, right: PdfRect) -> PdfRect {
    PdfRect {
        x0: left.x0.min(right.x0),
        y0: left.y0.min(right.y0),
        x1: left.x1.max(right.x1),
        y1: left.y1.max(right.y1),
    }
}

fn median_rect_extent(glyphs: &[HighlightGlyph], extent: RectExtent) -> f32 {
    let mut values: Vec<f32> = glyphs
        .iter()
        .map(|glyph| match extent {
            RectExtent::Width => glyph.bbox.width(),
            RectExtent::Height => glyph.bbox.height(),
        })
        .filter(|value| *value > 0.0)
        .collect();
    if values.is_empty() {
        return 1.0;
    }

    values.sort_by(|left, right| left.total_cmp(right));
    values[values.len() / 2]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RectExtent {
    Width,
    Height,
}

#[cfg(test)]
mod tests {
    use super::merge_text_glyph_rects;
    use crate::backend::{PdfRect, TextGlyph};

    #[test]
    fn merges_same_line_with_spaces() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('b', 20.0, 20.0, 28.0, 32.0),
            glyph(' ', 29.0, 20.0, 33.0, 32.0),
            glyph('c', 34.0, 20.0, 42.0, 32.0),
            glyph('d', 44.0, 20.0, 52.0, 32.0),
        ];

        let rects = merge_text_glyph_rects(&glyphs);

        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 52.0,
                y1: 32.0
            }
        );
    }

    #[test]
    fn splits_wrapped_horizontal_lines() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('b', 20.0, 20.0, 28.0, 32.0),
            glyph('c', 30.0, 20.0, 38.0, 32.0),
            glyph('d', 10.0, 36.0, 18.0, 48.0),
            glyph('e', 20.0, 36.0, 28.0, 48.0),
            glyph('f', 30.0, 36.0, 38.0, 48.0),
        ];

        let rects = merge_text_glyph_rects(&glyphs);

        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 38.0,
                y1: 32.0
            }
        );
        assert_eq!(
            rects[1],
            PdfRect {
                x0: 10.0,
                y0: 36.0,
                x1: 38.0,
                y1: 48.0
            }
        );
    }

    #[test]
    fn merges_vertical_column() {
        let glyphs = vec![
            glyph('縦', 80.0, 10.0, 92.0, 22.0),
            glyph('書', 80.0, 24.0, 92.0, 36.0),
            glyph('き', 80.0, 38.0, 92.0, 50.0),
        ];

        let rects = merge_text_glyph_rects(&glyphs);

        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 80.0,
                y0: 10.0,
                x1: 92.0,
                y1: 50.0
            }
        );
    }

    #[test]
    fn splits_wrapped_vertical_columns() {
        let glyphs = vec![
            glyph('縦', 80.0, 10.0, 92.0, 22.0),
            glyph('書', 80.0, 24.0, 92.0, 36.0),
            glyph('き', 80.0, 38.0, 92.0, 50.0),
            glyph('折', 62.0, 10.0, 74.0, 22.0),
            glyph('返', 62.0, 24.0, 74.0, 36.0),
            glyph('し', 62.0, 38.0, 74.0, 50.0),
        ];

        let rects = merge_text_glyph_rects(&glyphs);

        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 80.0,
                y0: 10.0,
                x1: 92.0,
                y1: 50.0
            }
        );
        assert_eq!(
            rects[1],
            PdfRect {
                x0: 62.0,
                y0: 10.0,
                x1: 74.0,
                y1: 50.0
            }
        );
    }

    fn glyph(ch: char, x0: f32, y0: f32, x1: f32, y1: f32) -> TextGlyph {
        TextGlyph {
            ch,
            bbox: Some(PdfRect { x0, y0, x1, y1 }),
        }
    }
}
