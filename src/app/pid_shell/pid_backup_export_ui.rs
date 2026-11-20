//! Headless `iced_test` verification of `backup_export_ui` (`UI-7` & `UI-8`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::{Point, Rectangle, Size};

use super::backup::SCREEN_BOUNDS_PREFIX;
use super::backup_export_ui::{self, ExportDialog, ExportUiMessage};
use super::*;
use crate::cli::GuiConfig;
use crate::io::pid_backup_browse::{
    BackupFile, BackupIndex, DrawingEntry, FileKind, ManifestSummary,
};
use crate::ui::window::pid_backup_browser::LoadState;

const WINDOW: Size = Size::new(1600.0, 1000.0);

fn app_on_backup() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.default_assoc_prompted = true;
    app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
    let first = app.open_first_screen(&GuiConfig::default());
    app.drive_headless_task(first)
        .expect("first screen opens headlessly");
    let _ = app.update(AppMessage::WindowResized(WINDOW.width, WINDOW.height));
    assert!(app.show_pid_screen(PidScreen::Backup));
    app
}

fn headless() -> iced::Renderer {
    use iced_test::core::renderer;
    iced_test::futures::futures::executor::block_on(<iced::Renderer as renderer::Headless>::new(
        renderer::Settings::default(),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("headless renderer")
}

fn drawn(app: &OpenCADStudio) -> BTreeMap<String, Rectangle> {
    use iced_test::core::{mouse, renderer};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    crate::ui::wrap_bar::forget_bounds("");
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), WINDOW, Cache::default(), &mut renderer);
    ui.draw(
        &mut renderer,
        &app.active_theme,
        &renderer::Style {
            text_color: iced::Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );
    crate::ui::wrap_bar::recorded_bounds().into_iter().collect()
}

#[allow(dead_code)]
fn says(app: &OpenCADStudio, needle: &str) -> bool {
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), WINDOW, Cache::default(), &mut renderer);
    let mut find = Selector::find(needle);
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    matches!(
        widget::Operation::finish(&find),
        widget::operation::Outcome::Some(Some(_))
    )
}

fn click_at(app: &mut OpenCADStudio, point: Point) {
    use iced_test::core::{mouse, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut renderer = headless();
    let mut cache = Cache::default();
    let events = [
        iced::Event::Mouse(mouse::Event::CursorMoved { position: point }),
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    for event in events {
        let mut messages = Vec::new();
        {
            let mut ui = UserInterface::build(app.view_main(), WINDOW, cache, &mut renderer);
            let _ = ui.update(
                &core_window::Headless,
                &shell::Waker::noop(),
                &[event],
                mouse::Cursor::Available(point),
                &mut renderer,
                &mut messages,
            );
            cache = ui.into_cache();
        }
        for message in messages {
            // Do NOT drive background export tasks here when testing running state;
            // synchronous UI state transitions happen inside `app.update(message)`.
            let _ = app.update(message);
        }
    }
}

fn send_export(app: &mut OpenCADStudio, msg: ExportUiMessage) {
    let _ = app.update(AppMessage::PidShell(Message::Backup(
        super::backup::Message::Export(msg),
    )));
}

fn temp_scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "ocs-pid-backup-export-ui-{}-{}-{}",
        tag,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn pid_backup_export_ui_ui7_loose_pid_disables_publish_but_allows_pdf() {
    let _one = crate::headless_ui_lock();
    let scratch = temp_scratch("ui7-loose");
    let pid_file = scratch.join("L01.pid");
    std::fs::write(&pid_file, b"loose-pid").unwrap();

    let mut app = app_on_backup();
    app.pid_backup_browser.load = LoadState::Ready(Arc::new(BackupIndex {
        source: scratch.clone(),
        restored: scratch.clone(),
        backup_root: scratch.clone(),
        read_only: true,
        manifest: None,
        drawings: vec![DrawingEntry {
            name: "L01".to_owned(),
            pid_path: Some(pid_file),
            group_path: "loose".to_owned(),
            meta: Vec::new(),
        }],
        ..BackupIndex::default()
    }));

    assert!(backup_export_ui::publish_disabled_reason(&app)
        .is_some_and(|why| why.contains("Manifest.txt")));
    assert_eq!(backup_export_ui::pdf_disabled_reason(&app), None);

    let bounds = drawn(&app);
    assert!(bounds.contains_key(&format!(
        "{SCREEN_BOUNDS_PREFIX}export_publish_disabled_hint"
    )));
    click_at(
        &mut app,
        bounds[&format!("{SCREEN_BOUNDS_PREFIX}export_publish")].center(),
    );
    assert!(app.pid_shell.backup.export_ui.dialog.is_none());

    click_at(
        &mut app,
        bounds[&format!("{SCREEN_BOUNDS_PREFIX}export_pdf")].center(),
    );
    assert!(matches!(
        app.pid_shell.backup.export_ui.dialog,
        Some(ExportDialog::Pdf(_))
    ));

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn pid_backup_export_ui_ui7_publish_dialog_prechecks_all_invalid_inputs_and_sql_warning() {
    let _one = crate::headless_ui_lock();
    let scratch = temp_scratch("ui7-pub-check");
    let pkg = scratch.join("pkg");
    let restore = scratch.join("restore");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::create_dir_all(&restore).unwrap();
    std::fs::write(pkg.join("Manifest.txt"), "[General]\r\nName=SqlP\r\n").unwrap();
    let pid_file = restore.join("A01.pid");
    std::fs::write(&pid_file, b"pid").unwrap();

    let non_empty = scratch.join("non-empty");
    std::fs::create_dir_all(&non_empty).unwrap();
    std::fs::write(non_empty.join("blocking.txt"), b"x").unwrap();

    let mut app = app_on_backup();
    app.pid_backup_browser.load = LoadState::Ready(Arc::new(BackupIndex {
        source: pkg.clone(),
        restored: restore.clone(),
        backup_root: pkg.clone(),
        read_only: false,
        manifest: Some(ManifestSummary {
            name: Some("SqlP".to_owned()),
            ..ManifestSummary::default()
        }),
        files: vec![BackupFile {
            name: "Export.dmp".to_owned(),
            kind: FileKind::ExportDump,
            format: "SQL Server MTF".to_owned(),
            ..BackupFile::default()
        }],
        drawings: vec![DrawingEntry {
            name: "A01".to_owned(),
            pid_path: Some(pid_file),
            group_path: "SqlP".to_owned(),
            meta: Vec::new(),
        }],
        ..BackupIndex::default()
    }));

    let bounds = drawn(&app);
    click_at(
        &mut app,
        bounds[&format!("{SCREEN_BOUNDS_PREFIX}export_publish")].center(),
    );
    let dlg_bounds = drawn(&app);
    assert!(dlg_bounds.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}publish_dialog")));
    assert!(dlg_bounds.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}sql_warning")));

    // 1. Non-empty folder -> SPPID_EXPORT_TARGET_NOT_EMPTY + blocking.txt, submit disabled.
    send_export(
        &mut app,
        ExportUiMessage::PublishOutChanged(non_empty.display().to_string()),
    );
    assert!(drawn(&app).contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pub_error")));
    send_export(&mut app, ExportUiMessage::SubmitPublish);
    assert!(!app.pid_backup_export.is_running());

    // 2. Folder inside package or restore -> SPPID_EXPORT_TARGET_INVALID.
    for inside in [pkg.join("sub"), restore.join("sub")] {
        send_export(
            &mut app,
            ExportUiMessage::PublishOutChanged(inside.display().to_string()),
        );
        assert!(drawn(&app).contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pub_error")));
        send_export(&mut app, ExportUiMessage::SubmitPublish);
        assert!(!app.pid_backup_export.is_running());
    }

    // 3. Empty plant name -> blocked.
    let valid_out = scratch.join("clean-out");
    send_export(
        &mut app,
        ExportUiMessage::PublishOutChanged(valid_out.display().to_string()),
    );
    send_export(
        &mut app,
        ExportUiMessage::PublishPlantChanged("   ".to_owned()),
    );
    assert!(drawn(&app).contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pub_error")));
    send_export(&mut app, ExportUiMessage::SubmitPublish);
    assert!(!app.pid_backup_export.is_running());

    // 4. Non-existent reference folder -> SPPID_EXPORT_REFERENCE_INVALID.
    send_export(
        &mut app,
        ExportUiMessage::PublishPlantChanged("P01".to_owned()),
    );
    send_export(
        &mut app,
        ExportUiMessage::PublishRefChanged(scratch.join("missing-ref").display().to_string()),
    );
    assert!(drawn(&app).contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pub_error")));
    send_export(&mut app, ExportUiMessage::SubmitPublish);
    assert!(!app.pid_backup_export.is_running());

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn pid_backup_export_ui_ui7_pdf_dialog_blocks_conflicts_and_pairs_with_last_publish_dir() {
    let _one = crate::headless_ui_lock();
    let scratch = temp_scratch("ui7-pdf-pair");
    let pkg = scratch.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("Manifest.txt"), "[General]\r\nName=P1\r\n").unwrap();
    let pid_file = pkg.join("A01.pid");
    std::fs::write(&pid_file, b"pid").unwrap();

    // Directory with conflicting _PdfIndex.json:
    let conflict_dir = scratch.join("conflict");
    std::fs::create_dir_all(&conflict_dir).unwrap();
    std::fs::write(conflict_dir.join("_PdfIndex.json"), b"{}").unwrap();

    // Directory with publish files only (paired delivery target!):
    let pair_dir = scratch.join("paired-deliver");
    std::fs::create_dir_all(&pair_dir).unwrap();
    std::fs::write(pair_dir.join("_Index.json"), b"{}").unwrap();
    std::fs::write(pair_dir.join("A01_Data.xml"), b"<xml/>").unwrap();

    let mut app = app_on_backup();
    app.pid_backup_browser.load = LoadState::Ready(Arc::new(BackupIndex {
        source: pkg.clone(),
        restored: pkg.clone(),
        backup_root: pkg.clone(),
        read_only: true,
        manifest: Some(ManifestSummary {
            name: Some("P1".to_owned()),
            ..ManifestSummary::default()
        }),
        files: vec![BackupFile {
            name: "Export.dmp".to_owned(),
            kind: FileKind::ExportDump,
            format: "SQL Server MTF".to_owned(),
            ..BackupFile::default()
        }],
        drawings: vec![DrawingEntry {
            name: "A01".to_owned(),
            pid_path: Some(pid_file),
            group_path: "P1".to_owned(),
            meta: Vec::new(),
        }],
        ..BackupIndex::default()
    }));
    app.pid_shell.backup.export_ui.last_publish_out_dir = Some(pair_dir.clone());

    let bounds = drawn(&app);
    click_at(
        &mut app,
        bounds[&format!("{SCREEN_BOUNDS_PREFIX}export_pdf")].center(),
    );
    let dlg = drawn(&app);
    assert!(dlg.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pdf_dialog")));
    assert!(dlg.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}sql_warning")));
    assert!(dlg.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pdf_use_last_publish")));

    // Conflict folder is rejected.
    send_export(
        &mut app,
        ExportUiMessage::PdfOutChanged(conflict_dir.display().to_string()),
    );
    assert!(drawn(&app).contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pdf_error")));
    send_export(&mut app, ExportUiMessage::SubmitPdf);
    assert!(!app.pid_backup_pdf.is_running());

    // Clicking "使用上次 publish 输出目录" fills `pair_dir`, shows pairing badge, and allows start!
    let dlg = drawn(&app);
    click_at(
        &mut app,
        dlg[&format!("{SCREEN_BOUNDS_PREFIX}pdf_use_last_publish")].center(),
    );
    let paired_dlg = drawn(&app);
    assert!(paired_dlg.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pdf_pair_ready")));
    assert!(!paired_dlg.contains_key(&format!("{SCREEN_BOUNDS_PREFIX}pdf_error")));
    click_at(
        &mut app,
        paired_dlg[&format!("{SCREEN_BOUNDS_PREFIX}pdf_submit")].center(),
    );
    assert!(app.pid_backup_pdf.is_running());
    assert!(app.pid_shell.backup.export_ui.dialog.is_none());

    let _ = std::fs::remove_dir_all(&scratch);
}
