
use std::fs;
use std::str::FromStr;
use std::time::Duration;

use super::summary::{build_aggregate_report, merge_stats, summarize_metric, summarize_scalar};
use super::{
    PerfIterationSnapshot, PerfScenarioId, PerfStats, PerfSuiteConfig, RedrawReason, run_suite,
    validate_suite_config,
};
use crate::backend::test_support::{build_pdf, unique_temp_path};

#[test]
fn records_milliseconds_and_clamped_rates() {
    let mut stats = PerfStats::default();
    stats.enable_sample_collection();
    stats.record_render(Duration::from_millis(12));
    stats.record_convert(Duration::from_millis(3));
    stats.record_blit(Duration::from_millis(1));
    stats.record_render_queue_wait(Duration::from_millis(4));
    stats.record_encode_queue_wait(Duration::from_millis(2));
    stats.set_l1_hit_rate(1.5);
    stats.set_l2_hit_rate(-0.5);
    stats.set_queue_depth(7);
    stats.set_render_in_flight(2);
    stats.set_encode_queue_depth(3);
    stats.set_encode_in_flight(1);
    stats.add_canceled_tasks(2);
    stats.add_encode_canceled_tasks(1);
    stats.record_redraw(RedrawReason::PendingWork);
    stats.record_redraw(RedrawReason::Timer);

    assert_eq!(stats.render_ms, 12.0);
    assert_eq!(stats.convert_ms, 3.0);
    assert_eq!(stats.blit_ms, 1.0);
    assert_eq!(stats.cache_hit_rate_l1, 1.0);
    assert_eq!(stats.cache_hit_rate_l2, 0.0);
    assert_eq!(stats.queue_depth, 7);
    assert_eq!(stats.render_in_flight, 2);
    assert_eq!(stats.encode_queue_depth, 3);
    assert_eq!(stats.encode_in_flight, 1);
    assert_eq!(stats.canceled_tasks, 2);
    assert_eq!(stats.render_canceled_tasks, 2);
    assert_eq!(stats.encode_canceled_tasks, 1);
    assert_eq!(stats.redraw_requests_total, 2);
    assert_eq!(stats.redraw_by_reason.pending_work, 1);
    assert_eq!(stats.redraw_by_reason.timer, 1);
}

#[test]
fn absorbs_presenter_metrics_without_overwriting_render_path() {
    let mut runtime = PerfStats::default();
    runtime.record_render(Duration::from_millis(11));

    let mut presenter = PerfStats::default();
    presenter.enable_sample_collection();
    presenter.record_convert(Duration::from_millis(5));
    presenter.record_blit(Duration::from_millis(2));
    presenter.record_encode_queue_wait(Duration::from_millis(3));
    presenter.set_l2_hit_rate(0.8);
    presenter.set_encode_queue_depth(4);
    presenter.set_encode_in_flight(1);
    presenter.add_encode_canceled_tasks(3);

    runtime.absorb_presenter_metrics(&presenter);

    assert_eq!(runtime.render_ms, 11.0);
    assert_eq!(runtime.convert_ms, 5.0);
    assert_eq!(runtime.blit_ms, 2.0);
    assert_eq!(runtime.cache_hit_rate_l2, 0.8);
    assert_eq!(runtime.encode_queue_depth, 4);
    assert_eq!(runtime.encode_in_flight, 1);
    assert_eq!(runtime.encode_canceled_tasks, 3);
    assert_eq!(runtime.encode_samples_ms(), &[5.0]);
    assert_eq!(runtime.blit_samples_ms(), &[2.0]);
    assert_eq!(runtime.encode_queue_wait_samples_ms(), &[3.0]);
    assert_eq!(runtime.encode_queue_depth_samples(), &[4]);
    assert_eq!(runtime.encode_in_flight_samples(), &[1]);
}

#[test]
fn summarizes_metrics_and_scalars() {
    let metric = summarize_metric(&[1.0, 2.0, 3.0, 4.0]);
    let scalar = summarize_scalar(&[1, 2, 3, 4]);

    assert_eq!(metric.count, 4);
    assert_eq!(metric.min_ms, 1.0);
    assert_eq!(metric.max_ms, 4.0);
    assert_eq!(scalar.count, 4);
    assert_eq!(scalar.min, 1.0);
    assert_eq!(scalar.max, 4.0);
}

#[test]
fn does_not_retain_samples_unless_enabled() {
    let mut stats = PerfStats::default();
    stats.record_render(Duration::from_millis(12));
    stats.record_convert(Duration::from_millis(3));
    stats.record_blit(Duration::from_millis(1));
    stats.record_render_queue_wait(Duration::from_millis(4));
    stats.record_encode_queue_wait(Duration::from_millis(2));
    stats.set_queue_depth(7);
    stats.set_render_in_flight(2);
    stats.set_encode_queue_depth(3);
    stats.set_encode_in_flight(1);

    assert_eq!(summarize_metric(stats.render_samples_ms()).count, 0);
    assert_eq!(summarize_metric(stats.encode_samples_ms()).count, 0);
    assert_eq!(summarize_metric(stats.blit_samples_ms()).count, 0);
    assert_eq!(
        summarize_scalar(stats.render_queue_depth_samples()).count,
        0
    );
    assert_eq!(
        summarize_scalar(stats.encode_queue_depth_samples()).count,
        0
    );
}

#[test]
fn parses_perf_scenarios() {
    for scenario in PerfScenarioId::all() {
        assert_eq!(PerfScenarioId::from_str(scenario.id()).unwrap(), *scenario);
    }
    assert!(PerfScenarioId::from_str("all").is_err());
    assert!(PerfScenarioId::from_str("unknown").is_err());
}

#[test]
fn resolves_page_step_policy_per_scenario() {
    let run = PerfSuiteConfig {
        page_steps: 7,
        ..PerfSuiteConfig::default()
    };

    assert_eq!(PerfScenarioId::ColdFirstPage.parameters(&run).page_steps, 0);
    assert_eq!(
        PerfScenarioId::IdleSettledRedraw
            .parameters(&run)
            .page_steps,
        0
    );
    assert_eq!(PerfScenarioId::ZoomStep.parameters(&run).page_steps, 2);
    assert_eq!(
        PerfScenarioId::SteadyNextPage.parameters(&run).page_steps,
        7
    );
    assert_eq!(
        PerfScenarioId::SteadyPrevPage.parameters(&run).page_steps,
        7
    );
    assert_eq!(PerfScenarioId::RapidNextPage.parameters(&run).page_steps, 7);
}

#[test]
fn merge_stats_averages_cache_hit_rates() {
    let mut first = PerfStats::default();
    first.set_l1_hit_rate(0.25);
    first.set_l2_hit_rate(0.5);

    let mut second = PerfStats::default();
    second.set_l1_hit_rate(0.75);
    second.set_l2_hit_rate(0.25);
    second.record_redraw(RedrawReason::Timer);

    let merged = merge_stats([&first, &second].into_iter());
    assert_eq!(merged.cache_hit_rate_l1, 0.5);
    assert_eq!(merged.cache_hit_rate_l2, 0.375);
    assert_eq!(merged.redraw_by_reason.timer, 1);
}

#[test]
fn clear_blit_metrics_drops_samples_and_current_value() {
    let mut stats = PerfStats::default();
    stats.enable_sample_collection();
    stats.record_blit(Duration::from_millis(2));

    stats.clear_blit_metrics();

    assert_eq!(stats.blit_ms, 0.0);
    assert_eq!(stats.blit_samples, 0);
    assert_eq!(summarize_metric(stats.blit_samples_ms()).count, 0);
}

#[test]
fn reset_clears_counters_and_samples() {
    let mut stats = PerfStats::default();
    stats.enable_sample_collection();
    stats.record_render(Duration::from_millis(12));
    stats.record_convert(Duration::from_millis(3));
    stats.record_blit(Duration::from_millis(1));
    stats.record_render_queue_wait(Duration::from_millis(4));
    stats.record_encode_queue_wait(Duration::from_millis(2));
    stats.set_queue_depth(7);
    stats.set_render_in_flight(2);
    stats.set_encode_queue_depth(3);
    stats.set_encode_in_flight(1);
    stats.add_canceled_tasks(2);
    stats.add_encode_canceled_tasks(1);
    stats.record_redraw(RedrawReason::Timer);

    stats.reset();

    assert_eq!(stats, PerfStats::default());
}

#[test]
fn rejects_zero_measured_iterations() {
    let run = PerfSuiteConfig {
        pdf_path: "sample.pdf".into(),
        measured_iterations: 0,
        ..PerfSuiteConfig::default()
    };

    let err = validate_suite_config(&run).expect_err("zero measured iterations should fail");
    assert!(err.to_string().contains("measured iteration"));
}

#[test]
fn allows_zero_page_steps_when_selected_scenarios_do_not_use_them() {
    let run = PerfSuiteConfig {
        pdf_path: "sample.pdf".into(),
        scenarios: vec![
            PerfScenarioId::ColdFirstPage,
            PerfScenarioId::ZoomStep,
            PerfScenarioId::IdleSettledRedraw,
        ],
        page_steps: 0,
        ..PerfSuiteConfig::default()
    };

    validate_suite_config(&run).expect("zero page steps should be allowed");
}

#[test]
fn rejects_zero_page_steps_when_selected_scenarios_use_them() {
    let run = PerfSuiteConfig {
        pdf_path: "sample.pdf".into(),
        scenarios: vec![PerfScenarioId::SteadyNextPage],
        page_steps: 0,
        ..PerfSuiteConfig::default()
    };

    let err = validate_suite_config(&run).expect_err("zero page steps should fail");
    assert!(err.to_string().contains("--page-steps"));
}

#[test]
fn rejects_iteration_count_overflow() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should initialize");
    let run = PerfSuiteConfig {
        pdf_path: "dummy.pdf".into(),
        warmup_iterations: usize::MAX,
        measured_iterations: 1,
        ..PerfSuiteConfig::default()
    };

    let err = runtime
        .block_on(run_suite(run))
        .expect_err("overflow should fail");
    assert!(err.to_string().contains("overflow"));
}

#[test]
fn aggregates_wall_phase_redraw_queue_and_cache_metrics() {
    let mut runtime = PerfStats::default();
    runtime.enable_sample_collection();
    runtime.record_render(Duration::from_millis(10));
    runtime.record_render_queue_wait(Duration::from_millis(2));
    runtime.set_queue_depth(3);
    runtime.set_render_in_flight(1);
    runtime.set_l1_hit_rate(0.5);
    runtime.record_redraw(RedrawReason::PendingWork);

    let mut presenter = PerfStats::default();
    presenter.enable_sample_collection();
    presenter.record_convert(Duration::from_millis(4));
    presenter.record_blit(Duration::from_millis(1));
    presenter.record_encode_queue_wait(Duration::from_millis(3));
    presenter.set_encode_queue_depth(2);
    presenter.set_encode_in_flight(1);
    presenter.set_l2_hit_rate(0.25);

    let aggregate = build_aggregate_report(&runtime, &presenter, &[12.0, 16.0]);

    assert_eq!(aggregate.wall_time_ms.count, 2);
    assert_eq!(aggregate.wall_time_ms.avg_ms, 14.0);
    assert_eq!(aggregate.phase_metrics.render_ms.count, 1);
    assert_eq!(aggregate.redraw.total, 1);
    assert_eq!(aggregate.queues.render_depth.max, 3.0);
    assert_eq!(aggregate.queues.encode_depth.max, 2.0);
    assert_eq!(aggregate.cache.l1_hit_rate, 0.5);
    assert_eq!(aggregate.cache.l2_hit_rate, 0.25);
}

#[test]
fn iteration_report_includes_wall_time_and_navigation_state() {
    let snapshot = PerfIterationSnapshot {
        runtime: PerfStats::default(),
        presenter: PerfStats::default(),
        wall_time: Duration::from_millis(7),
        final_page: 2,
        visited_steps: 3,
    };

    let report = snapshot.into_report(4);

    assert_eq!(report.iteration_index, 4);
    assert_eq!(report.wall_time_ms, 7.0);
    assert_eq!(report.final_page, 2);
    assert_eq!(report.visited_steps, 3);
}

#[test]
fn runtime_smoke_returns_all_scenarios() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should initialize");
    let file = unique_temp_path(".pdf");
    fs::write(&file, build_pdf(&["one", "two", "three"])).expect("test pdf should be written");

    let config = PerfSuiteConfig {
        pdf_path: file.clone(),
        warmup_iterations: 0,
        measured_iterations: 1,
        page_steps: 8,
        idle_ms: 1,
        ..PerfSuiteConfig::default()
    };
    let report = runtime
        .block_on(run_suite(config))
        .expect("suite should run");

    fs::remove_file(file).expect("test pdf should be removed");

    assert_eq!(report.scenarios.len(), PerfScenarioId::all().len());
    let rapid = report
        .scenarios
        .iter()
        .find(|scenario| scenario.id == PerfScenarioId::RapidNextPage)
        .expect("rapid scenario should be reported");
    assert!(rapid.iterations[0].final_page < 3);
    let idle = report
        .scenarios
        .iter()
        .find(|scenario| scenario.id == PerfScenarioId::IdleSettledRedraw)
        .expect("idle scenario should be reported");
    assert_eq!(idle.parameters.idle_duration_ms, 1);
}
