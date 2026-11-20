//! 备份数据包 — screen `backup`; in the design, frames `BkF01`..`BkF11`
//! (and their light counterparts `BkL01`..`BkL11`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::widget::{button, column, container, row, text, Space};
use iced::{alignment, font, Background, Border, Color, Element, Fill, Padding, Task};

use super::{Message as ShellMessage, PidScreen, ViewFn};
use crate::app::{Message as AppMessage, OpenCADStudio};
use crate::io::pid_backup_browse::{BackupIndex, FileKind};
use crate::ui::style::pid_fonts;
use crate::ui::style::pid_tokens::{ColorToken, NumberToken, PidTheme};
use crate::ui::window::pid_backup_browser::{CacheMsg, LoadState, PidBackupMsg};
use crate::ui::wrap_bar::PosReport;

/// Prefix of every bounds rectangle reported inside the `backup` screen.
pub const SCREEN_BOUNDS_PREFIX: &str = "PID_BACKUP_SCREEN:";

fn part_id(name: &str) -> String {
    format!("{SCREEN_BOUNDS_PREFIX}{name}")
}

/// Injectable source picker for headless tests (`true` = folder, `false` = zip).
/// Returning `Some(Some(path))` loads `path`; `Some(None)` cancels; `None`
/// falls back to the native file dialog.
pub type SourcePicker = Arc<dyn Fn(bool) -> Option<Option<PathBuf>> + Send + Sync>;

/// Tone of an inline notice shown in the `backup` screen header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeTone {
    Info,
    Warning,
}

/// What the screen remembers between frames.
#[derive(Default)]
pub struct State {
    /// Inline notice from the last secondary action (e.g. cache clear).
    pub notice: Option<(NoticeTone, String)>,
    /// Optional test double replacing the native source picker dialog.
    pub source_picker: Option<SourcePicker>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("notice", &self.notice)
            .field("has_source_picker", &self.source_picker.is_some())
            .finish()
    }
}

/// What the screen's widgets send.
#[derive(Debug, Clone)]
pub enum Message {
    /// Pick and open a `.zip` (`false`) or folder (`true`) backup source.
    PickSource { folder: bool },
    /// Clear restored backup caches except the currently open one (S3).
    ClearCache,
    /// Switch to the editor and reveal the classic docked backup panel (S3).
    OpenClassicPanel,
}

/// The function that draws the screen.
pub(crate) const VIEW: Option<ViewFn> = Some(view);

/// Whether `index` holds a SQL Server MTF database dump.
pub fn is_sql_server_package(index: &BackupIndex) -> bool {
    index.files.iter().any(|file| {
        file.kind == FileKind::ExportDump && file.format.to_ascii_lowercase().contains("sql server")
    })
}

/// Summary title line of an open backup package.
pub fn header_title_line(index: &BackupIndex) -> String {
    if let Some(manifest) = &index.manifest {
        let plant = manifest
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("未命名工厂");
        let mut tags = Vec::new();
        if let Some(version) = manifest.version.as_deref().filter(|v| !v.trim().is_empty()) {
            tags.push(format!("版本 {version}"));
        }
        if let Some(kind) = manifest
            .backup_type
            .as_deref()
            .filter(|k| !k.trim().is_empty())
        {
            tags.push(format!("类型 {kind}"));
        }
        if let Some(date) = manifest
            .date_created
            .as_deref()
            .filter(|d| !d.trim().is_empty())
        {
            tags.push(date.to_owned());
        }
        if is_sql_server_package(index) {
            tags.push("SQL Server 备份".to_owned());
        }
        if tags.is_empty() {
            format!("备份数据包 · {plant}")
        } else {
            format!("备份数据包 · {plant}（{}）", tags.join(" · "))
        }
    } else {
        let folder = index
            .source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| index.source.display().to_string());
        format!("备份数据包 · {folder}（散放图纸目录，无备份清单）")
    }
}

/// Summary metadata line of an open backup package.
pub fn header_meta_line(index: &BackupIndex) -> String {
    let mode = if index.read_only {
        "直接使用目录（只读）"
    } else {
        "已还原到缓存目录"
    };
    format!(
        "{} · {} · {} 张图纸 · {} 个包内文件 · {} 个参考数据文件 · {} 条警告",
        index.source.display(),
        mode,
        index.drawings.len(),
        index.files.len(),
        index.ref_tree.file_count(),
        index.warnings.len(),
    )
}

/// Take one of the screen's messages.
pub(crate) fn update(app: &mut OpenCADStudio, message: Message) -> Task<AppMessage> {
    match message {
        Message::PickSource { folder } => {
            app.pid_shell.backup.notice = None;
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(picker) = app.pid_shell.backup.source_picker.clone() {
                    match picker(folder) {
                        Some(Some(path)) => return app.load_pid_backup(path, None),
                        Some(None) => return Task::none(),
                        None => {}
                    }
                }
                app.pick_backup_source(folder)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = folder;
                Task::none()
            }
        }
        Message::ClearCache => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let busy = app.pid_backup_export.is_running() || app.pid_backup_pdf.is_running();
                let task = app.on_pid_backup(PidBackupMsg::Cache(CacheMsg::Clear));
                if busy {
                    app.pid_shell.backup.notice = Some((
                        NoticeTone::Warning,
                        "正在后台导出备份数据包，导出完成前暂不清理还原缓存。".to_owned(),
                    ));
                } else {
                    app.pid_shell.backup.notice = Some((
                        NoticeTone::Info,
                        "已清理非当前打开备份包的还原缓存。".to_owned(),
                    ));
                }
                task
            }
            #[cfg(target_arch = "wasm32")]
            Task::none()
        }
        Message::OpenClassicPanel => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if app.tabs.get(app.active_tab).is_some_and(|tab| tab.is_start) {
                    let _ = app.update(AppMessage::TabNew);
                }
                app.show_pid_backup_panel();
                let _ = app.show_pid_screen(PidScreen::Editor);
            }
            Task::none()
        }
    }
}

fn card_style(theme: PidTheme, fill: ColorToken, stroke: ColorToken) -> container::Style {
    container::Style {
        background: Some(Background::Color(fill.color(theme))),
        border: Border {
            color: stroke.color(theme),
            width: 1.0,
            radius: NumberToken::RadiusMd.value(theme).into(),
        },
        ..container::Style::default()
    }
}

fn action_button<'a>(
    theme: PidTheme,
    bounds_key: &'static str,
    label: &'static str,
    primary: bool,
    on_press: Message,
) -> Element<'a, AppMessage> {
    let (bg, fg, stroke, weight) = if primary {
        (
            ColorToken::Accent,
            ColorToken::TextOnAccent,
            ColorToken::Accent,
            font::Weight::Semibold,
        )
    } else {
        (
            ColorToken::BgRaised,
            ColorToken::TextPrimary,
            ColorToken::BorderStrong,
            font::Weight::Medium,
        )
    };
    let radius = NumberToken::RadiusSm.value(theme);
    let btn = button(
        text(label)
            .size(12)
            .font(pid_fonts::ui(theme, weight))
            .color(fg.color(theme)),
    )
    .padding(Padding::from([7, 12]))
    .style(move |_, status| {
        let fill = match status {
            button::Status::Hovered | button::Status::Pressed if !primary => {
                ColorToken::BgHover.color(theme)
            }
            _ => bg.color(theme),
        };
        button::Style {
            background: Some(Background::Color(fill)),
            text_color: fg.color(theme),
            border: Border {
                color: stroke.color(theme),
                width: 1.0,
                radius: radius.into(),
            },
            shadow: iced::Shadow::default(),
            snap: false,
        }
    })
    .on_press(AppMessage::PidShell(ShellMessage::Backup(on_press)));

    PosReport::owned(part_id(bounds_key), btn).into()
}

fn empty_view<'a>(theme: PidTheme) -> Element<'a, AppMessage> {
    let card = container(
        column![
            text("打开 SmartPlant P&ID 备份数据包")
                .size(18)
                .font(pid_fonts::ui(theme, font::Weight::Semibold))
                .color(ColorToken::TextPrimary.color(theme)),
            text("选择本机 .zip 备份包或备份目录，按装置组浏览图纸、查看数据库字段与图内对象，并导出成对移交的 publish 文件与 PDF（无需连接协同服务器）。")
                .size(12.5)
                .font(pid_fonts::ui(theme, font::Weight::Normal))
                .color(ColorToken::TextSecondary.color(theme)),
            row![
                action_button(
                    theme,
                    "open_zip",
                    "打开备份 zip",
                    true,
                    Message::PickSource { folder: false }
                ),
                action_button(
                    theme,
                    "open_folder",
                    "打开备份文件夹",
                    true,
                    Message::PickSource { folder: true }
                ),
            ]
            .spacing(10),
        ]
        .spacing(14),
    )
    .width(580)
    .padding([28, 32])
    .style(move |_| card_style(theme, ColorToken::BgPanel, ColorToken::Border));

    PosReport::owned(
        part_id("empty"),
        container(card)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .padding(32),
    )
    .into()
}

fn loading_view<'a>(
    app: &'a OpenCADStudio,
    source: &'a Path,
    theme: PidTheme,
) -> Element<'a, AppMessage> {
    let secs = app
        .pid_backup_browser
        .loading_elapsed(iced::time::Instant::now())
        .map_or(0, |elapsed| elapsed.as_secs());
    let detail = format!("来源：{} · 已用 {secs} 秒", source.display());
    let card = container(
        column![
            text("正在还原并建立索引…")
                .size(18)
                .font(pid_fonts::ui(theme, font::Weight::Semibold))
                .color(ColorToken::TextPrimary.color(theme)),
            text(detail)
                .size(12.5)
                .font(pid_fonts::mono(theme, font::Weight::Normal))
                .color(ColorToken::TextSecondary.color(theme)),
        ]
        .spacing(12),
    )
    .width(580)
    .padding([28, 32])
    .style(move |_| card_style(theme, ColorToken::BgPanel, ColorToken::Accent));

    PosReport::owned(
        part_id("loading"),
        container(card)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .padding(32),
    )
    .into()
}

fn error_view<'a>(
    app: &'a OpenCADStudio,
    reason: &'a str,
    theme: PidTheme,
) -> Element<'a, AppMessage> {
    let src_line = app
        .pid_backup_browser
        .active_or_failed_source()
        .map(|src| format!("来源：{}", src.display()));
    let mut body = column![text("无法打开所选备份数据包")
        .size(18)
        .font(pid_fonts::ui(theme, font::Weight::Semibold))
        .color(ColorToken::Danger.color(theme)),]
    .spacing(12);
    if let Some(src_line) = src_line {
        body = body.push(
            text(src_line)
                .size(12)
                .font(pid_fonts::mono(theme, font::Weight::Normal))
                .color(ColorToken::TextSecondary.color(theme)),
        );
    }
    body = body.push(
        text(reason)
            .size(12.5)
            .font(pid_fonts::ui(theme, font::Weight::Normal))
            .color(ColorToken::TextPrimary.color(theme)),
    );
    body = body.push(
        row![
            action_button(
                theme,
                "retry_zip",
                "重新选择 zip",
                true,
                Message::PickSource { folder: false }
            ),
            action_button(
                theme,
                "retry_folder",
                "重新选择文件夹",
                true,
                Message::PickSource { folder: true }
            ),
        ]
        .spacing(10),
    );

    let card = container(body)
        .width(600)
        .padding([28, 32])
        .style(move |_| card_style(theme, ColorToken::DangerSoft, ColorToken::Danger));

    PosReport::owned(
        part_id("error"),
        container(card)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .padding(32),
    )
    .into()
}

fn package_header<'a>(index: &'a BackupIndex, theme: PidTheme) -> Element<'a, AppMessage> {
    let titles = column![
        text(header_title_line(index))
            .size(15)
            .font(pid_fonts::ui(theme, font::Weight::Semibold))
            .color(ColorToken::TextPrimary.color(theme)),
        text(header_meta_line(index))
            .size(11.5)
            .font(pid_fonts::ui(theme, font::Weight::Normal))
            .color(ColorToken::TextSecondary.color(theme)),
    ]
    .spacing(4);

    let actions = row![
        action_button(
            theme,
            "switch_zip",
            "切换 zip",
            false,
            Message::PickSource { folder: false }
        ),
        action_button(
            theme,
            "switch_folder",
            "切换文件夹",
            false,
            Message::PickSource { folder: true }
        ),
        action_button(
            theme,
            "clear_cache",
            "清理还原缓存",
            false,
            Message::ClearCache
        ),
        action_button(
            theme,
            "classic_panel",
            "在经典面板中查看库表",
            false,
            Message::OpenClassicPanel
        ),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center);

    let bar = container(
        row![titles, Space::new().width(Fill), actions]
            .spacing(12)
            .align_y(alignment::Vertical::Center),
    )
    .width(Fill)
    .padding([14, 20])
    .style(move |_| container::Style {
        background: Some(Background::Color(ColorToken::BgPanel.color(theme))),
        border: Border {
            color: ColorToken::Border.color(theme),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    });

    PosReport::owned(part_id("header"), bar).into()
}

fn ready_view<'a>(
    app: &'a OpenCADStudio,
    index: &'a BackupIndex,
    theme: PidTheme,
) -> Element<'a, AppMessage> {
    let mut root = column![package_header(index, theme)];
    if let Some((tone, msg)) = &app.pid_shell.backup.notice {
        let (bg, fg) = match tone {
            NoticeTone::Info => (ColorToken::AccentSoft, ColorToken::TextPrimary),
            NoticeTone::Warning => (ColorToken::AmberSoft, ColorToken::Amber),
        };
        root = root.push(
            container(
                text(msg.as_str())
                    .size(12)
                    .font(pid_fonts::ui(theme, font::Weight::Medium))
                    .color(fg.color(theme)),
            )
            .width(Fill)
            .padding([8, 20])
            .style(move |_| container::Style {
                background: Some(Background::Color(bg.color(theme))),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }),
        );
    }
    root = root.push(
        container(
            text(format!("已载入 {} 张图纸", index.drawings.len()))
                .size(12.5)
                .font(pid_fonts::ui(theme, font::Weight::Normal))
                .color(ColorToken::TextSecondary.color(theme)),
        )
        .width(Fill)
        .height(Fill)
        .padding([16, 20]),
    );
    root.width(Fill).height(Fill).into()
}

/// Draw the backup package screen inside the shell's content area.
pub(crate) fn view(app: &OpenCADStudio) -> Element<'_, AppMessage> {
    let theme = app.pid_shell.theme;
    let body = match &app.pid_backup_browser.load {
        LoadState::Idle => match app.pid_backup_browser.error.as_deref() {
            Some(reason) => error_view(app, reason, theme),
            None => empty_view(theme),
        },
        LoadState::Loading(source) => loading_view(app, source, theme),
        LoadState::Ready(index) => ready_view(app, index, theme),
    };
    container(body)
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(ColorToken::BgApp.color(theme))),
            ..container::Style::default()
        })
        .into()
}
