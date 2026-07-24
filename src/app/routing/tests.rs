use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Size;
use tokio::runtime::Builder;

use crate::app::App;
use crate::app::core::InteractionSubsystem;
use crate::app::runtime::{ActiveDocument, RuntimeControl, RuntimeEvent};
use crate::app::terminal_session::{TerminalSession, TerminalSurface};
use crate::app::{
    RuntimeDriver, RuntimeDriverDecision, RuntimeDriverHandle, RuntimeMetricsSnapshot,
    RuntimeObservation,
};
use crate::backend::test_support::{build_pdf, unique_temp_path};
use crate::backend::{OutlineNode, PdfBackend, PdfDoc, RgbaFrame, SharedPdfBackend, TextPage};
use crate::command::{Command, CommandInvocationSource, CommandRequest, PanAmount, PanDirection};
use crate::condition::ConditionExpr;
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::event::{
    DocumentReloadReason, DocumentReloadRequest, DocumentReloadResult, DomainEvent,
};
use crate::input::sequence::SequenceRegistry;
use crate::input::shortcut::ShortcutKey;
use crate::presenter::{PresenterBackgroundEvent, PresenterKind};
use crate::render::cache::RenderedPageKey;
use crate::render::worker::RenderWorkerResult;
use crate::work::WorkClass;

struct StubSession {
    size: Size,
    restore_count: Option<Arc<AtomicUsize>>,
}

impl StubSession {
    fn new(width: u16, height: u16) -> Self {
        Self {
            size: Size::new(width, height),
            restore_count: None,
        }
    }

    fn with_restore_count(width: u16, height: u16, restore_count: Arc<AtomicUsize>) -> Self {
        Self {
            size: Size::new(width, height),
            restore_count: Some(restore_count),
        }
    }
}

impl TerminalSurface for StubSession {
    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn draw<F>(&mut self, _render: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        Ok(())
    }
}

impl TerminalSession for StubSession {
    fn restore(&mut self) -> io::Result<()> {
        if let Some(count) = &self.restore_count {
            count.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct RestoreProbeDriver;

impl RuntimeDriver for RestoreProbeDriver {
    type Output = ();

    fn on_iteration(
        &mut self,
        _observation: RuntimeObservation,
        _handle: &mut RuntimeDriverHandle<'_>,
    ) -> AppResult<RuntimeDriverDecision> {
        Ok(RuntimeDriverDecision::Finish)
    }

    fn on_finish(
        &mut self,
        _observation: RuntimeObservation,
        _metrics: RuntimeMetricsSnapshot,
    ) -> AppResult<Self::Output> {
        Ok(())
    }

    fn on_break(&mut self) -> AppResult<Self::Output> {
        Ok(())
    }
}

struct EmptyPdfBackend {
    path: PathBuf,
}

impl EmptyPdfBackend {
    fn new() -> Self {
        Self {
            path: PathBuf::from("empty.pdf"),
        }
    }
}

impl PdfBackend for EmptyPdfBackend {
    fn path(&self) -> &Path {
        &self.path
    }

    fn doc_id(&self) -> u64 {
        0
    }

    fn page_count(&self) -> usize {
        0
    }

    fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
        Err(AppError::invalid_argument("empty pdf"))
    }

    fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
        Err(AppError::invalid_argument("empty pdf"))
    }

    fn extract_text_page(&self, _page: usize) -> AppResult<TextPage> {
        Err(AppError::invalid_argument("empty pdf"))
    }

    fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
        Ok(Vec::new())
    }
}

fn test_pdf_backend() -> SharedPdfBackend {
    let file = unique_temp_path(".pdf");
    fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");
    fs::remove_file(&file).expect("test pdf should be removed");
    Arc::new(doc)
}

#[test]
fn run_event_runtime_restores_session_when_pdf_has_no_pages() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let restore_count = Arc::new(AtomicUsize::new(0));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let session = StubSession::with_restore_count(80, 24, Arc::clone(&restore_count));
    let pdf: SharedPdfBackend = Arc::new(EmptyPdfBackend::new());
    let driver = RestoreProbeDriver;

    let err = tokio_runtime
        .block_on(app.run_event_runtime(pdf, session, crate::app::RuntimeMode::Headless, driver))
        .expect_err("empty pdf should fail before the loop starts");

    assert!(err.to_string().contains("pdf has no pages"));
    assert_eq!(restore_count.load(Ordering::Relaxed), 1);
}

fn failed_render_result(
    key: RenderedPageKey,
    class: WorkClass,
    generation: u64,
) -> RenderWorkerResult {
    RenderWorkerResult {
        key,
        class,
        generation,
        result: Err(crate::error::AppError::pdf_render(
            key.page,
            crate::error::AppError::invalid_argument("render failed"),
        )),
        queue_wait: Duration::from_millis(1),
        elapsed: Duration::from_millis(2),
    }
}

#[test]
fn resolve_command_uses_short_edge_fifth_for_default_pan_step() {
    let app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let session = StubSession::new(80, 24);
    let short_edge_cells = 23_u16;
    let expected_step = i32::from((short_edge_cells / 5).max(1));

    let resolved = app.resolve_command(
        &session,
        Command::Pan {
            direction: PanDirection::Right,
            amount: PanAmount::DefaultStep,
        },
    );

    assert_eq!(
        resolved,
        Command::Pan {
            direction: PanDirection::Right,
            amount: PanAmount::Cells(expected_step),
        }
    );
}

#[test]
fn resolve_command_clamps_default_pan_step_to_at_least_one_cell() {
    let app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let session = StubSession::new(20, 5);

    let resolved = app.resolve_command(
        &session,
        Command::Pan {
            direction: PanDirection::Down,
            amount: PanAmount::DefaultStep,
        },
    );

    assert_eq!(
        resolved,
        Command::Pan {
            direction: PanDirection::Down,
            amount: PanAmount::Cells(1),
        }
    );
}

#[test]
fn resolve_command_request_preserves_explicit_pan_amounts() {
    let app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let session = StubSession::new(80, 24);

    let resolved = app.resolve_command_request(
        &session,
        CommandRequest::new(
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(3),
            },
            crate::command::CommandInvocationSource::CommandPaletteInput,
        ),
    );

    assert_eq!(
        resolved.command,
        Command::Pan {
            direction: PanDirection::Left,
            amount: PanAmount::Cells(3),
        }
    );
}

#[test]
fn wake_timeout_applies_expired_sequence_command() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let mut registry = SequenceRegistry::new();
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('g')],
            Command::OpenHelp,
        )
        .expect("single-key binding should register");
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('g'), ShortcutKey::char('g')],
            Command::FirstPage,
        )
        .expect("multi-key binding should register");
    app.interaction =
        InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

    app.interaction
        .handle_key_event(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        )
        .expect("first key should be captured");

    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Wake),
            &mut runtime,
            &mut document,
        )
        .expect("wake should be handled");

    assert!(matches!(control, RuntimeControl::Continue));
    assert_eq!(app.state.mode, crate::app::Mode::Help);
    assert!(!matches!(
        runtime.event_rx.try_recv(),
        Ok(DomainEvent::Command(_))
    ));
}

#[test]
fn input_outcome_applies_expired_command_before_latest_command() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let mut registry = SequenceRegistry::new();
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('g')],
            Command::DebugStatusShow,
        )
        .expect("single-key binding should register");
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('g'), ShortcutKey::char('g')],
            Command::LastPage,
        )
        .expect("multi-key binding should register");
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('x')],
            Command::DebugStatusHide,
        )
        .expect("single-key binding should register");
    app.interaction =
        InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

    app.interaction
        .handle_key_event(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        )
        .expect("first key should be captured");

    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
            )))),
            &mut runtime,
            &mut document,
        )
        .expect("input should be handled");

    assert!(matches!(control, RuntimeControl::Continue));
    assert!(
        !app.state.debug_status_visible,
        "DebugStatusShow must be applied before DebugStatusHide"
    );
    assert!(!matches!(
        runtime.event_rx.try_recv(),
        Ok(DomainEvent::Command(_))
    ));
}

#[test]
fn focus_changing_timeout_drops_waited_input() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let mut registry = SequenceRegistry::new();
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('x')],
            Command::OpenHelp,
        )
        .expect("single-key binding should register");
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('x'), ShortcutKey::char('x')],
            Command::LastPage,
        )
        .expect("multi-key binding should register");
    registry
        .register_exact(
            ConditionExpr::Always,
            &[ShortcutKey::char('j')],
            Command::HelpScrollDown,
        )
        .expect("help binding should register");
    app.interaction =
        InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

    app.interaction
        .handle_key_event(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        )
        .expect("first key should be captured");

    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Char('j'),
                KeyModifiers::NONE,
            )))),
            &mut runtime,
            &mut document,
        )
        .expect("input should be handled");

    assert!(matches!(control, RuntimeControl::Continue));
    assert_eq!(app.state.mode, crate::app::Mode::Help);
    assert_eq!(app.state.help_scroll, 0);
}

#[test]
fn palette_close_from_input_applies_before_queued_input() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    app.interaction
        .palette
        .pending_requests
        .push_back(crate::app::PaletteRequest::Open {
            kind: crate::palette::PaletteKind::Command,
            options: crate::palette::PaletteOpenOptions::default(),
        });
    assert!(app.interaction.apply_palette_requests(&mut app.state));
    assert_eq!(app.state.mode, crate::app::Mode::Palette);
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime
        .event_tx
        .send(DomainEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))))
        .expect("queued input should be accepted");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            )))),
            &mut runtime,
            &mut document,
        )
        .expect("palette close input should be handled");

    assert!(matches!(control, RuntimeControl::Continue));
    assert_eq!(app.state.mode, crate::app::Mode::Normal);
    assert!(matches!(
        runtime.event_rx.try_recv(),
        Ok(DomainEvent::Input(Event::Key(key)))
            if key.code == KeyCode::Char('x')
    ));
}

#[test]
fn command_error_becomes_notice_and_runtime_continues() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::GotoPage { page: 999 },
                CommandInvocationSource::Binding,
            ))),
            &mut runtime,
            &mut document,
        )
        .expect("command error should be handled as a notice");

    assert!(matches!(control, RuntimeControl::Continue));
    let notice = app.state.notice.expect("command error should set a notice");
    assert_eq!(notice.level, crate::app::NoticeLevel::Warning);
    assert_eq!(notice.message, "page 999 is out of range (1-1)");
    assert!(runtime.event_rx.try_recv().is_err());
}

#[test]
fn reload_document_command_starts_reload_without_blocking_loop() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::ReloadDocument,
                CommandInvocationSource::CommandPaletteInput,
            ))),
            &mut runtime,
            &mut document,
        )
        .expect("reload command should be handled");

    assert!(matches!(control, RuntimeControl::Continue));
    assert!(runtime.reload_in_flight);
}

#[test]
fn document_reload_success_replaces_active_document_and_clamps_page() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let file = unique_temp_path("reload_success.pdf");
    fs::write(&file, build_pdf(&["one", "two", "three"]))
        .expect("first test pdf should be created");
    let first = Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
    let old_doc_id = first.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&first));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    app.state.current_page = 2;
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&first),
            first.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();

    fs::write(&file, build_pdf(&["new one", "new two"]))
        .expect("second test pdf should replace first");
    let second = Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
    assert_ne!(old_doc_id, second.doc_id());

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::Manual,
            generation: 0,
            result: Ok(second),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload result should be handled");

    assert_ne!(document.pdf.doc_id(), old_doc_id);
    assert_eq!(runtime.page_count, 2);
    assert_eq!(app.state.current_page, 1);
    assert!(runtime.ui_actor.needs_redraw());
    fs::remove_file(&file).expect("test file should be removed");
}

#[test]
fn document_reload_success_applies_even_when_doc_id_matches() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let file = unique_temp_path("reload_same_doc_id.pdf");
    fs::write(&file, build_pdf(&["same content"])).expect("test pdf should be created");
    let first = Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
    let second = Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
    assert_eq!(first.doc_id(), second.doc_id());
    assert!(!Arc::ptr_eq(&first, &second));

    let mut document = ActiveDocument::new(Arc::clone(&first));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&first),
            first.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;
    runtime.reload_retry_attempts = 2;

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::Manual,
            generation: 0,
            result: Ok(Arc::clone(&second)),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload result should be handled");

    assert!(Arc::ptr_eq(&document.pdf, &second));
    assert_eq!(runtime.reload_retry_attempts, 0);
    assert!(runtime.ui_actor.needs_redraw());
    fs::remove_file(&file).expect("test file should be removed");
}

#[test]
fn document_reload_success_clears_previous_reload_notice() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let file = unique_temp_path("reload_clears_notice.pdf");
    fs::write(&file, build_pdf(&["one"])).expect("first test pdf should be created");
    let first = Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
    let mut document = ActiveDocument::new(Arc::clone(&first));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    app.state
        .set_warning_notice("Could not reload changed document: still invalid");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&first),
            first.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.reload_in_flight = true;

    fs::write(&file, build_pdf(&["two"])).expect("second test pdf should replace first");
    let second = Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Ok(second),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload result should be handled");

    assert!(app.state.notice.is_none());
    fs::remove_file(&file).expect("test file should be removed");
}

#[test]
fn manual_document_reload_failure_keeps_previous_document() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let old_doc_id = pdf.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::Manual,
            generation: 0,
            result: Err("still being written".to_string()),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload failure should be handled");

    assert_eq!(document.pdf.doc_id(), old_doc_id);
    assert!(!runtime.reload_in_flight);
    let notice = app.state.notice.expect("reload failure should set notice");
    assert_eq!(notice.level, crate::app::NoticeLevel::Error);
    assert!(notice.message.contains("still being written"));
}

#[test]
fn file_reload_failure_keeps_previous_document_and_retries_quietly() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let old_doc_id = pdf.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Err("still being written".to_string()),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload failure should be handled");

    assert_eq!(document.pdf.doc_id(), old_doc_id);
    assert!(!runtime.reload_in_flight);
    assert_eq!(runtime.reload_retry_attempts, 1);
    assert!(app.state.notice.is_none());

    let retry = tokio_runtime
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(1), runtime.event_rx.recv()).await
        })
        .expect("retry event should arrive")
        .expect("loop event channel should stay open");
    assert!(matches!(
        retry,
        DomainEvent::ReloadDocument(DocumentReloadRequest {
            reason: DocumentReloadReason::FileChanged,
            retry: true,
            ..
        })
    ));
}

#[test]
fn file_reload_failure_after_retry_budget_shows_warning() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let old_doc_id = pdf.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;
    runtime.reload_retry_attempts = 5;

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Err("still invalid after retries".to_string()),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload failure should be handled");

    assert_eq!(document.pdf.doc_id(), old_doc_id);
    assert!(!runtime.reload_in_flight);
    assert_eq!(runtime.reload_retry_attempts, 5);
    assert!(runtime.ui_actor.needs_redraw());
    let notice = app.state.notice.expect("reload failure should set notice");
    assert_eq!(notice.level, crate::app::NoticeLevel::Warning);
    assert!(notice.message.contains("still invalid after retries"));
    assert!(runtime.event_rx.try_recv().is_err());
}

#[test]
fn file_reload_success_after_retries_replaces_document_and_resets_retry_count() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let file = unique_temp_path("reload_retry_success.pdf");
    fs::write(&file, build_pdf(&["one", "two", "three"]))
        .expect("first test pdf should be created");
    let first = Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
    let old_doc_id = first.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&first));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    app.state.current_page = 2;
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&first),
            first.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;
    runtime.reload_retry_attempts = 3;

    fs::write(&file, build_pdf(&["new one", "new two"]))
        .expect("second test pdf should replace first");
    let second = Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
    assert_ne!(old_doc_id, second.doc_id());

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Ok(second),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload result should be handled");

    assert_ne!(document.pdf.doc_id(), old_doc_id);
    assert_eq!(runtime.page_count, 2);
    assert_eq!(app.state.current_page, 1);
    assert_eq!(runtime.reload_retry_attempts, 0);
    assert!(app.state.notice.is_none());
    assert!(runtime.ui_actor.needs_redraw());
    fs::remove_file(&file).expect("test file should be removed");
}

#[test]
fn stale_file_reload_failure_yields_to_pending_fresh_reload() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let old_doc_id = pdf.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;
    runtime.reload_retry_attempts = 2;
    runtime.pending_reload = Some(DocumentReloadRequest::new(
        DocumentReloadReason::FileChanged,
    ));

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Err("stale failure".to_string()),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload failure should be handled");

    assert_eq!(document.pdf.doc_id(), old_doc_id);
    assert!(runtime.reload_in_flight);
    assert!(runtime.pending_reload.is_none());
    assert_eq!(runtime.reload_retry_attempts, 0);
    assert!(app.state.notice.is_none());
    assert!(!runtime.ui_actor.needs_redraw());
}

#[test]
fn stale_file_reload_success_yields_to_pending_fresh_reload() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let file = unique_temp_path("reload_stale_success.pdf");
    fs::write(&file, build_pdf(&["one", "two", "three"]))
        .expect("first test pdf should be created");
    let first = Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
    let old_doc_id = first.doc_id();
    let mut document = ActiveDocument::new(Arc::clone(&first));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&first),
            first.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    runtime.reload_in_flight = true;
    runtime.reload_retry_attempts = 2;
    runtime.pending_reload = Some(DocumentReloadRequest::new(
        DocumentReloadReason::FileChanged,
    ));

    fs::write(&file, build_pdf(&["stale one", "stale two"]))
        .expect("second test pdf should replace first");
    let stale = Arc::new(PdfDoc::open(&file).expect("stale pdf should open")) as SharedPdfBackend;
    assert_ne!(old_doc_id, stale.doc_id());

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: DocumentReloadReason::FileChanged,
            generation: 0,
            result: Ok(stale),
        })),
        &mut runtime,
        &mut document,
    )
    .expect("reload result should be handled");

    assert_eq!(document.pdf.doc_id(), old_doc_id);
    assert!(runtime.reload_in_flight);
    assert!(runtime.pending_reload.is_none());
    assert_eq!(runtime.reload_retry_attempts, 0);
    assert!(app.state.notice.is_none());
    assert!(!runtime.ui_actor.needs_redraw());

    runtime.event_bus.shutdown();
    fs::remove_file(&file).expect("test file should be removed");
}

#[test]
fn old_delayed_retry_after_newer_reload_is_ignored() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.reload_generation = 2;
    runtime.reload_retry_attempts = 3;
    runtime.ui_actor.clear_redraw();

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::ReloadDocument(DocumentReloadRequest::retry(
            DocumentReloadReason::FileChanged,
            1,
        ))),
        &mut runtime,
        &mut document,
    )
    .expect("stale retry should be handled");

    assert!(!runtime.reload_in_flight);
    assert!(runtime.pending_reload.is_none());
    assert_eq!(runtime.reload_generation, 2);
    assert_eq!(runtime.reload_retry_attempts, 3);
    assert!(app.state.notice.is_none());
    assert!(!runtime.ui_actor.needs_redraw());
}

#[test]
fn command_event_returns_break_when_effect_channel_is_closed() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.event_rx.close();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    let control = app
        .handle_waited_event(
            RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::NextPage,
                CommandInvocationSource::Binding,
            ))),
            &mut runtime,
            &mut document,
        )
        .expect("command should be handled");

    assert!(matches!(control, RuntimeControl::Break));
}

#[test]
fn encode_complete_without_redraw_request_does_not_redraw() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    runtime.ui_actor.clear_redraw();
    let mut document = ActiveDocument::new(Arc::clone(&pdf));

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::EncodeComplete(
            PresenterBackgroundEvent::EncodeComplete {
                redraw_requested: false,
            },
        )),
        &mut runtime,
        &mut document,
    )
    .expect("encode completion should be handled");

    assert!(!runtime.ui_actor.needs_redraw());
}

#[test]
fn prefetch_tick_only_marks_prefetch_due() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    assert!(runtime.render_actor.take_prefetch_due());
    assert!(!runtime.render_actor.take_prefetch_due());
    runtime.ui_actor.clear_redraw();

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::PrefetchTick),
        &mut runtime,
        &mut document,
    )
    .expect("prefetch tick should be handled");

    assert!(runtime.render_actor.take_prefetch_due());
    assert!(!runtime.ui_actor.needs_redraw());
    assert!(runtime.event_rx.try_recv().is_err());
}

#[test]
fn non_current_render_complete_does_not_redraw() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    runtime.ui_actor.clear_redraw();
    let viewport = App::current_viewport(&runtime.session, app.state.debug_status_visible);
    let current_scale = app.compute_current_scale(pdf.as_ref(), app.state.current_page, viewport);
    let non_current_key = RenderedPageKey::new(pdf.doc_id(), 42, current_scale);

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::RenderComplete(failed_render_result(
            non_current_key,
            WorkClass::Background,
            runtime.render_actor.generation(),
        ))),
        &mut runtime,
        &mut document,
    )
    .expect("render completion should be handled");

    assert!(!runtime.ui_actor.needs_redraw());
    assert!(app.state.notice.is_none());
}

#[test]
fn noop_navigation_command_without_state_change_does_not_redraw() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    runtime.ui_actor.clear_redraw();

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
            Command::PrevPage,
            CommandInvocationSource::Binding,
        ))),
        &mut runtime,
        &mut document,
    )
    .expect("command should be handled");

    assert!(!runtime.ui_actor.needs_redraw());
    let event = match runtime.event_rx.try_recv() {
        Ok(DomainEvent::App(event)) => event,
        other => panic!("expected command event, got {other:?}"),
    };
    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::App(event)),
        &mut runtime,
        &mut document,
    )
    .expect("app event should be handled");
    assert!(!runtime.ui_actor.needs_redraw());
}

#[test]
fn unavailable_search_navigation_without_notice_change_does_not_redraw() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    runtime.ui_actor.clear_redraw();

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
            Command::NextSearchHit,
            CommandInvocationSource::Binding,
        ))),
        &mut runtime,
        &mut document,
    )
    .expect("command should be handled");

    assert!(!runtime.ui_actor.needs_redraw());
}

#[test]
fn noop_command_redraws_when_it_changes_visible_notice() {
    let tokio_runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let _guard = tokio_runtime.enter();
    let pdf = test_pdf_backend();
    let mut app =
        App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
    app.state.set_warning_notice("old warning");
    let (event_tx, event_rx, event_bus) = crate::app::event_bus::EventBusRuntime::spawn_headless();
    let session = StubSession::new(80, 24);
    let mut runtime = app
        .initialize_runtime(
            Arc::clone(&pdf),
            pdf.page_count(),
            session,
            event_tx,
            event_rx,
            event_bus,
        )
        .expect("runtime should initialize");
    let mut document = ActiveDocument::new(Arc::clone(&pdf));
    runtime.ui_actor.clear_redraw();

    app.handle_waited_event(
        RuntimeEvent::Event(DomainEvent::Command(CommandRequest::new(
            Command::PrevPage,
            CommandInvocationSource::Binding,
        ))),
        &mut runtime,
        &mut document,
    )
    .expect("command should be handled");

    assert!(app.state.notice.is_none());
    assert!(runtime.ui_actor.needs_redraw());
}
