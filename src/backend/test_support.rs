use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_temp_path(suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    let mut path = std::env::temp_dir();
    std::fs::create_dir_all(&path).expect("test temp directory should be created");
    path.push(format!("pvf_{suffix}_{}_{}", process::id(), nanos));
    path
}

pub(crate) fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
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

pub(crate) fn build_pdf_with_raw_streams(page_streams: &[&str]) -> Vec<u8> {
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

    build_pdf_from_objects(&objects)
}

pub(crate) fn build_pdf_from_objects(objects: &[String]) -> Vec<u8> {
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
