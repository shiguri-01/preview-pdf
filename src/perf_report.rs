use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use tokio::time::sleep;

use crate::app::RenderRuntime;
use crate::backend::open_default_backend;
use crate::error::{AppError, AppResult};
use crate::perf::{PerfStats, PhaseSummary, RedrawReason, TimeSeriesSample};
use crate::presenter::{
    ImagePresenter, PanOffset, PresenterFeedback, PresenterRenderOptions, RatatuiImagePresenter,
    Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::PrefetchClass;
use crate::render::scheduler::{NavDirection, NavIntent, RenderPriority, RenderTask};
use crate::render::worker::RenderWorker;

const SCENARIO_NAME: &str = "fixed-navigation-v1";
const SCENARIO_ITERATIONS: usize = 2;
const DRAW_RETRY_DELAY: Duration = Duration::from_millis(5);
const DRAW_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfReportFormat {
    Json,
    Csv,
}

impl PerfReportFormat {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            other => Err(AppError::invalid_argument(format!(
                "unsupported perf report format: {other} (expected json or csv)"
            ))),
        }
    }
}

pub async fn run_fixed_perf_report(path: &Path, format: PerfReportFormat) -> AppResult<String> {
    let started = Instant::now();
    let mut pdf = open_default_backend(path)?;
    let page_count = pdf.page_count();
    if page_count == 0 {
        return Err(AppError::invalid_argument("pdf has no pages"));
    }

    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 100,
        height: 40,
    };
    let mut terminal = Terminal::new(TestBackend::new(viewport.width, viewport.height))
        .expect("test backend terminal should initialize");
    let mut runtime = RenderRuntime::default();
    let mut presenter = RatatuiImagePresenter::new();
    let mut render_worker = RenderWorker::spawn(path.to_path_buf(), pdf.doc_id(), 1);
    let pages = scenario_pages(page_count);
    let scales = [1.0_f32, 1.35_f32];
    let mut cached_keys = Vec::new();

    for iteration in 0..SCENARIO_ITERATIONS {
        for (offset, page) in pages.iter().copied().enumerate() {
            let scale = scales[(iteration + offset) % scales.len()];
            let generation = (iteration * pages.len() + offset + 1) as u64;
            let nav = NavIntent {
                dir: if offset % 2 == 0 {
                    NavDirection::Forward
                } else {
                    NavDirection::Backward
                },
                streak: offset + 1,
                generation,
            };

            runtime.schedule_navigation(pdf.as_mut(), page, nav, scale);
            runtime
                .perf_stats
                .record_redraw_request(RedrawReason::Input);

            let key = RenderedPageKey::new(pdf.doc_id(), page, scale);
            if !runtime.has_cached_frame(&key) {
                enqueue_current_task(
                    &mut runtime,
                    &mut render_worker,
                    RenderTask {
                        doc_id: pdf.doc_id(),
                        page,
                        scale,
                        priority: RenderPriority::CriticalCurrent,
                        generation,
                        reason: "perf-scenario-current",
                    },
                )
                .await?;
                let completed = render_worker.recv_result().await.ok_or_else(|| {
                    AppError::unsupported("render worker closed during perf scenario")
                })?;
                runtime
                    .perf_stats
                    .record_render_wait(completed.wait_elapsed);
                runtime.ingest_rendered_frame(completed.key, completed.result?, completed.elapsed, true);
                runtime.set_queue_depth_with_inflight(render_worker.in_flight_len());
                runtime
                    .perf_stats
                    .record_redraw_request(RedrawReason::Completion);
            }

            if !cached_keys.contains(&key) {
                cached_keys.push(key);
            }

            let mut pan = PanOffset::default();
            runtime.try_prepare_current_page_from_cache(
                pdf.as_mut(),
                &mut presenter,
                viewport,
                page,
                scale,
                &mut pan,
                None,
                false,
                generation,
            )?;

            for cached_key in cached_keys
                .iter()
                .copied()
                .filter(|cached_key| *cached_key != key)
                .take(2)
            {
                let mut prefetch_pan = PanOffset::default();
                let _ = runtime.try_prefetch_encode_from_cache(
                    &mut presenter,
                    viewport,
                    cached_key,
                    &mut prefetch_pan,
                    None,
                    false,
                    PrefetchClass::DirectionalLead,
                    generation,
                )?;
            }

            draw_until_ready(&mut terminal, &mut presenter, &mut runtime, viewport).await?;
        }
    }

    let report = PerfScenarioReport {
        scenario: SCENARIO_NAME,
        pdf_path: path.to_path_buf(),
        page_count,
        iterations: SCENARIO_ITERATIONS * pages.len(),
        wall_time_ms: started.elapsed().as_secs_f64() * 1000.0,
        stats: runtime.perf_stats.clone(),
    };

    Ok(match format {
        PerfReportFormat::Json => report.to_json(),
        PerfReportFormat::Csv => report.to_csv(),
    })
}

async fn enqueue_current_task(
    runtime: &mut RenderRuntime,
    render_worker: &mut RenderWorker,
    task: RenderTask,
) -> AppResult<()> {
    let deadline = Instant::now() + DRAW_TIMEOUT;
    while !render_worker.enqueue(task.clone()) {
        runtime.set_queue_depth_with_inflight(render_worker.in_flight_len());
        if Instant::now() >= deadline {
            return Err(AppError::unsupported(
                "timed out waiting for render worker slot during perf scenario",
            ));
        }
        sleep(DRAW_RETRY_DELAY).await;
    }
    runtime.set_queue_depth_with_inflight(render_worker.in_flight_len());
    Ok(())
}

async fn draw_until_ready(
    terminal: &mut Terminal<TestBackend>,
    presenter: &mut RatatuiImagePresenter,
    runtime: &mut RenderRuntime,
    viewport: Viewport,
) -> AppResult<()> {
    let deadline = Instant::now() + DRAW_TIMEOUT;
    loop {
        let mut feedback = PresenterFeedback::Pending;
        let mut drew_image = false;
        terminal
            .draw(|frame| {
                let outcome = presenter
                    .render(
                        frame,
                        Rect::new(0, 0, viewport.width, viewport.height),
                        PresenterRenderOptions::default(),
                    )
                    .expect("perf scenario presenter render should succeed");
                feedback = outcome.feedback;
                drew_image = outcome.drew_image;
            })
            .expect("test backend draw should succeed");
        runtime.sync_presenter_metrics(presenter);
        runtime.perf_stats.record_frame_draw();

        match feedback {
            PresenterFeedback::None if drew_image => return Ok(()),
            PresenterFeedback::Failed => {
                return Err(AppError::unsupported(
                    "perf scenario presenter failed to render image",
                ));
            }
            PresenterFeedback::Pending | PresenterFeedback::None => {}
        }

        if presenter.drain_background_events() {
            runtime.sync_presenter_metrics(presenter);
            runtime
                .perf_stats
                .record_redraw_request(RedrawReason::Completion);
        }
        runtime
            .perf_stats
            .record_redraw_request(RedrawReason::Timer);

        if Instant::now() >= deadline {
            return Err(AppError::unsupported(
                "timed out waiting for presenter completion during perf scenario",
            ));
        }
        sleep(DRAW_RETRY_DELAY).await;
    }
}

fn scenario_pages(page_count: usize) -> Vec<usize> {
    let mut pages = BTreeSet::new();
    pages.insert(0);
    pages.insert(page_count / 2);
    pages.insert(page_count.saturating_sub(1));
    pages.into_iter().collect()
}

struct PerfScenarioReport {
    scenario: &'static str,
    pdf_path: PathBuf,
    page_count: usize,
    iterations: usize,
    wall_time_ms: f64,
    stats: PerfStats,
}

impl PerfScenarioReport {
    fn to_json(&self) -> String {
        let render = self.stats.render_summary();
        let convert = self.stats.convert_summary();
        let blit = self.stats.blit_summary();
        let render_wait = self.stats.render_wait_summary();
        let encode_wait = self.stats.encode_wait_summary();
        let redraw = self.stats.redraw_summary();
        format!(
            concat!(
                "{{\n",
                "  \"scenario\": \"{}\",\n",
                "  \"pdf_path\": \"{}\",\n",
                "  \"page_count\": {},\n",
                "  \"iterations\": {},\n",
                "  \"wall_time_ms\": {:.3},\n",
                "  \"phases\": {{\n",
                "{}",
                "  }},\n",
                "  \"redraw\": {{\n",
                "    \"input\": {},\n",
                "    \"completion\": {},\n",
                "    \"timer\": {},\n",
                "    \"frames_drawn\": {}\n",
                "  }},\n",
                "  \"samples\": {{\n",
                "    \"queue_depth\": {},\n",
                "    \"in_flight\": {},\n",
                "    \"canceled_tasks\": {}\n",
                "  }}\n",
                "}}\n"
            ),
            self.scenario,
            escape_json_string(&self.pdf_path.display().to_string()),
            self.page_count,
            self.iterations,
            self.wall_time_ms,
            join_phase_json([
                ("render", render),
                ("convert", convert),
                ("blit", blit),
                ("render_wait", render_wait),
                ("encode_wait", encode_wait),
            ]),
            redraw.input,
            redraw.completion,
            redraw.timer,
            redraw.frames_drawn,
            samples_to_json(self.stats.queue_depth_samples()),
            samples_to_json(self.stats.in_flight_samples()),
            samples_to_json(self.stats.canceled_task_samples()),
        )
    }

    fn to_csv(&self) -> String {
        let mut rows = Vec::new();
        rows.push(
            "section,name,string_value,at_ms,value,count,latest_ms,avg_ms,p50_ms,p95_ms,p99_ms"
                .to_string(),
        );
        rows.push(csv_row([
            "meta".to_string(),
            "scenario".to_string(),
            self.scenario.to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]));
        rows.push(format!(
            "{}",
            csv_row([
                "meta".to_string(),
                "pdf_path".to_string(),
                self.pdf_path.display().to_string(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ])
        ));
        rows.push(csv_row([
            "meta".to_string(),
            "page_count".to_string(),
            String::new(),
            String::new(),
            self.page_count.to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]));
        rows.push(csv_row([
            "meta".to_string(),
            "iterations".to_string(),
            String::new(),
            String::new(),
            self.iterations.to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]));
        rows.push(csv_row([
            "meta".to_string(),
            "wall_time_ms".to_string(),
            String::new(),
            String::new(),
            format!("{:.3}", self.wall_time_ms),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]));

        for (name, summary) in [
            ("render", self.stats.render_summary()),
            ("convert", self.stats.convert_summary()),
            ("blit", self.stats.blit_summary()),
            ("render_wait", self.stats.render_wait_summary()),
            ("encode_wait", self.stats.encode_wait_summary()),
        ] {
            rows.push(phase_csv_row(name, summary));
        }

        let redraw = self.stats.redraw_summary();
        rows.push(redraw_csv_row("input", redraw.input));
        rows.push(redraw_csv_row("completion", redraw.completion));
        rows.push(redraw_csv_row("timer", redraw.timer));
        rows.push(redraw_csv_row("frames_drawn", redraw.frames_drawn));

        append_sample_rows("queue_depth", self.stats.queue_depth_samples(), &mut rows);
        append_sample_rows("in_flight", self.stats.in_flight_samples(), &mut rows);
        append_sample_rows("canceled_tasks", self.stats.canceled_task_samples(), &mut rows);
        rows.push(String::new());
        rows.join("\n")
    }
}

fn join_phase_json(phases: [(&str, PhaseSummary); 5]) -> String {
    phases
        .into_iter()
        .enumerate()
        .map(|(index, (name, summary))| {
            let suffix = if index == 4 { "\n" } else { ",\n" };
            format!(
                "    \"{name}\": {{ \"count\": {}, \"latest_ms\": {:.3}, \"avg_ms\": {:.3}, \"p50_ms\": {:.3}, \"p95_ms\": {:.3}, \"p99_ms\": {:.3} }}{suffix}",
                summary.count,
                summary.latest_ms,
                summary.avg_ms,
                summary.p50_ms,
                summary.p95_ms,
                summary.p99_ms,
            )
        })
        .collect()
}

fn samples_to_json(samples: &[TimeSeriesSample]) -> String {
    let values = samples
        .iter()
        .map(|sample| {
            format!(
                "{{ \"at_ms\": {:.3}, \"value\": {} }}",
                sample.at_ms, sample.value
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn phase_csv_row(name: &str, summary: PhaseSummary) -> String {
    csv_row([
        "phase".to_string(),
        name.to_string(),
        String::new(),
        String::new(),
        String::new(),
        summary.count.to_string(),
        format!("{:.3}", summary.latest_ms),
        format!("{:.3}", summary.avg_ms),
        format!("{:.3}", summary.p50_ms),
        format!("{:.3}", summary.p95_ms),
        format!("{:.3}", summary.p99_ms),
    ])
}

fn redraw_csv_row(name: &str, value: u64) -> String {
    csv_row([
        "redraw".to_string(),
        name.to_string(),
        String::new(),
        String::new(),
        value.to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ])
}

fn append_sample_rows(name: &str, samples: &[TimeSeriesSample], rows: &mut Vec<String>) {
    for sample in samples {
        rows.push(csv_row([
            "sample".to_string(),
            name.to_string(),
            String::new(),
            format!("{:.3}", sample.at_ms),
            sample.value.to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]));
    }
}

fn csv_row(fields: [String; 11]) -> String {
    fields
        .into_iter()
        .map(|field| escape_csv(&field))
        .collect::<Vec<_>>()
        .join(",")
}

fn escape_json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{PerfReportFormat, run_fixed_perf_report};

    #[tokio::test(flavor = "current_thread")]
    async fn fixed_perf_report_outputs_json_with_percentiles() {
        let file = unique_temp_path("perf_report.json.pdf");
        fs::write(&file, build_pdf(&["one", "two", "three"])).expect("test pdf should exist");

        let report = run_fixed_perf_report(&file, PerfReportFormat::Json)
            .await
            .expect("perf report should succeed");

        assert!(report.contains("\"scenario\": \"fixed-navigation-v1\""));
        assert!(report.contains("\"p95_ms\""));
        assert!(report.contains("\"redraw\""));

        fs::remove_file(&file).expect("test pdf should be removed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fixed_perf_report_finishes_under_loose_threshold() {
        let file = unique_temp_path("perf_report_threshold.pdf");
        fs::write(&file, build_pdf(&["one", "two"])).expect("test pdf should exist");

        let started = Instant::now();
        let _ = run_fixed_perf_report(&file, PerfReportFormat::Csv)
            .await
            .expect("perf report should succeed");
        assert!(started.elapsed() < Duration::from_secs(10));

        fs::remove_file(&file).expect("test pdf should be removed");
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("pvf-{}-{}-{suffix}", process::id(), nanos));
        path
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_streams = page_texts
            .iter()
            .map(|text| format!("BT /F1 14 Tf 36 260 Td ({}) Tj ET", escape_pdf_text(text)))
            .collect::<Vec<_>>();
        build_pdf_from_streams(&page_streams)
    }

    fn escape_pdf_text(text: &str) -> String {
        text.replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }

    fn build_pdf_from_streams(page_streams: &[String]) -> Vec<u8> {
        let page_count = page_streams.len();
        let pages_obj = 2;
        let font_obj = 3;
        let first_page_obj = 4;
        let mut objects = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
        let kids = (0..page_count)
            .map(|index| format!("{} 0 R", first_page_obj + index * 2))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            page_count, kids
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
        for (index, stream) in page_streams.iter().enumerate() {
            let page_obj = first_page_obj + index * 2;
            let contents_obj = page_obj + 1;
            objects.push(format!(
                "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 {} 0 R >> >> /Contents {} 0 R >>",
                pages_obj, font_obj, contents_obj
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ));
        }

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );
        pdf
    }
}
