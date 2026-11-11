//! The shell as it is drawn: the three bars and their sizes, the navigation,
//! what the bars say about the collaboration session, what can be pressed —
//! and the general drawing interface inside it, which has to behave in its
//! frame exactly as it does in a window of its own.

use std::collections::BTreeMap;

use iced::{Point, Rectangle, Size};
use serde_json::Value;

use super::chrome::{self, SyncStatus};
use super::*;
use crate::cli::GuiConfig;
use crate::ui::style::pid_tokens::ColorToken;

/// The design's frame.
const WINDOW: Size = Size::new(1600.0, 1000.0);
/// What the shell leaves of it for the screen.
const CONTENT: Size = Size::new(
    WINDOW.width - chrome::NAV_WIDTH,
    WINDOW.height - chrome::TITLE_BAR_HEIGHT - chrome::STATUS_BAR_HEIGHT,
);
/// Where the content area starts in the window.
const ORIGIN: iced::Vector = iced::Vector::new(chrome::NAV_WIDTH, chrome::TITLE_BAR_HEIGHT);

// ── An application in a window ───────────────────────────────────────────

/// The application as a launch leaves it once the startup prompts have been
/// answered, in a window of `window`: in the shell, or with `--classic`.
fn launched(classic: bool, window: Size) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.default_assoc_prompted = true;
    app.donation_prompt_version = env!("OCS_APP_VERSION").to_owned();
    let cfg = GuiConfig {
        classic,
        new: classic,
        ..GuiConfig::default()
    };
    let first = app.open_first_screen(&cfg);
    app.drive_headless_task(first)
        .expect("the first screen opens without a window");
    let _ = app.update(AppMessage::WindowResized(window.width, window.height));
    app
}

/// The shell in the design's frame.
fn shell() -> OpenCADStudio {
    launched(false, WINDOW)
}

/// The general interface on its own, in a window the size of the shell's
/// content area: what the editor inside the shell has to match.
fn alone() -> OpenCADStudio {
    launched(true, CONTENT)
}

fn headless() -> iced::Renderer {
    use iced_test::core::renderer;
    iced_test::futures::futures::executor::block_on(<iced::Renderer as renderer::Headless>::new(
        renderer::Settings::default(),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("headless renderer")
}

/// Draw the window once; where every reporting widget is, in window
/// coordinates.
fn drawn(app: &OpenCADStudio, window: Size) -> BTreeMap<String, Rectangle> {
    use iced_test::core::{mouse, renderer};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    crate::ui::wrap_bar::forget_bounds("");
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), window, Cache::default(), &mut renderer);
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

/// Whether the text `label` is in the window.
fn says(app: &OpenCADStudio, window: Size, label: &str) -> bool {
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), window, Cache::default(), &mut renderer);
    let mut find = Selector::find(label);
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    matches!(
        widget::Operation::finish(&find),
        widget::operation::Outcome::Some(Some(_))
    )
}

/// Feed pointer events to the window one at a time, the pointer being where
/// each event says, and give the application what the widgets publish — as
/// the runtime does. The messages are returned as well.
fn pointer(app: &mut OpenCADStudio, window: Size, steps: &[(Point, Pointer)]) -> Vec<String> {
    use iced_test::core::{mouse, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut renderer = headless();
    let mut cache = Cache::default();
    let mut published = Vec::new();
    for (at, step) in steps {
        let event = iced::Event::Mouse(match step {
            Pointer::Move => mouse::Event::CursorMoved { position: *at },
            Pointer::Press(button) => mouse::Event::ButtonPressed(*button),
            Pointer::Release(button) => mouse::Event::ButtonReleased(*button),
        });
        let mut messages = Vec::new();
        {
            let mut ui = UserInterface::build(app.view_main(), window, cache, &mut renderer);
            let _ = ui.update(
                &core_window::Headless,
                &shell::Waker::noop(),
                std::slice::from_ref(&event),
                mouse::Cursor::Available(*at),
                &mut renderer,
                &mut messages,
            );
            cache = ui.into_cache();
        }
        for message in messages {
            published.push(format!("{message:?}"));
            let _ = app.update(message);
        }
    }
    published
}

#[derive(Clone, Copy)]
enum Pointer {
    Move,
    Press(iced::mouse::Button),
    Release(iced::mouse::Button),
}

/// A click with the left button at `at`.
fn click(at: Point) -> [(Point, Pointer); 3] {
    [
        (at, Pointer::Move),
        (at, Pointer::Press(iced::mouse::Button::Left)),
        (at, Pointer::Release(iced::mouse::Button::Left)),
    ]
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

// ── The three bars ───────────────────────────────────────────────────────

const PEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/pid-platform/design/pid-platform.pen"
);

/// The first node called `name` under `node`.
fn node_named<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    if node["name"] == name {
        return Some(node);
    }
    node["children"]
        .as_array()?
        .iter()
        .find_map(|child| node_named(child, name))
}

/// The sizes are the design's: read from the `.pen`, and what the window is
/// actually divided by.
#[test]
fn the_three_bars_have_the_sizes_the_design_gives_them() {
    let text = std::fs::read_to_string(PEN).unwrap_or_else(|e| panic!("{PEN}: {e}"));
    let pen: Value = serde_json::from_str(&text).expect("the design is JSON");
    let shell_node = pen["children"]
        .as_array()
        .expect("top-level nodes")
        .iter()
        .find(|node| node["name"] == "component/App Shell")
        .expect("the shell component");
    let designed = |name: &str, side: &str| {
        node_named(shell_node, name).unwrap_or_else(|| panic!("no {name}"))[side]
            .as_f64()
            .unwrap_or_else(|| panic!("{name} has no {side}")) as f32
    };
    assert_eq!(chrome::TITLE_BAR_HEIGHT, designed("Title Bar", "height"));
    assert_eq!(chrome::NAV_WIDTH, designed("Nav", "width"));
    assert_eq!(chrome::STATUS_BAR_HEIGHT, designed("Status Bar", "height"));
    assert_eq!(
        (WINDOW.width, WINDOW.height),
        (
            shell_node["width"].as_f64().expect("a width") as f32,
            shell_node["height"].as_f64().expect("a height") as f32
        ),
        "the frame these tests lay the window out in is the design's"
    );

    let _one = crate::headless_ui_lock();
    let app = shell();
    let at = drawn(&app, WINDOW);
    assert_eq!(at["PID_CHROME:title"], rect(0.0, 0.0, 1600.0, 44.0));
    assert_eq!(at["PID_CHROME:nav"], rect(0.0, 44.0, 208.0, 930.0));
    assert_eq!(at["PID_CHROME:status"], rect(0.0, 974.0, 1600.0, 26.0));
    assert_eq!(at["PID_SHELL:editor"], rect(208.0, 44.0, 1392.0, 930.0));
    // What the general interface measures by is that area.
    assert_eq!(app.win_size, (CONTENT.width, CONTENT.height));
}

// ── The navigation ───────────────────────────────────────────────────────

#[test]
fn the_navigation_draws_the_implemented_screens_under_the_designs_groups() {
    // The groups hold exactly what `nav()` lists, in its order.
    let groups = chrome::nav_groups();
    let listed: Vec<PidScreen> = nav().collect();
    let grouped: Vec<PidScreen> = groups
        .iter()
        .flat_map(|(_, screens)| screens.iter().copied())
        .collect();
    assert_eq!(grouped, listed);
    for (group, screens) in &groups {
        assert!(!screens.is_empty());
        assert!(screens.iter().all(|screen| screen.group() == *group));
    }

    let _one = crate::headless_ui_lock();
    let app = shell();
    let at = drawn(&app, WINDOW);
    let nav_area = at["PID_CHROME:nav"];
    for screen in PidScreen::ALL {
        let entry = at.get(&chrome::nav_bounds_id(screen));
        assert_eq!(
            entry.is_some(),
            screen.is_implemented(),
            "{screen:?} has an entry exactly when it is implemented"
        );
        if let Some(entry) = entry {
            assert!(
                entry.x >= nav_area.x && entry.x + entry.width <= nav_area.x + nav_area.width,
                "{screen:?} is in the navigation: {entry:?}"
            );
            assert!(says(&app, WINDOW, screen.title()), "{screen:?} is labelled");
        }
    }
    // A group is drawn exactly when one of its screens is.
    for group in [
        NavGroup::Design,
        NavGroup::Config,
        NavGroup::Delivery,
        NavGroup::Platform,
    ] {
        let has_screen = groups.iter().any(|(drawn, _)| *drawn == group);
        assert_eq!(says(&app, WINDOW, group.title()), has_screen, "{group:?}");
    }
}

// ── The collaboration session ────────────────────────────────────────────

/// Six states, each brought about in the session itself, each said in the
/// navigation's foot in its own words and its own colour.
#[test]
fn the_sync_status_says_what_the_session_is() {
    let _one = crate::headless_ui_lock();
    let mut app = shell();
    let mut seen = Vec::new();
    let mut check = |app: &OpenCADStudio, expected: SyncStatus, label: &str, dot: ColorToken| {
        let status = SyncStatus::of(app);
        assert_eq!(status, expected);
        assert_eq!(status.label(), label);
        assert_eq!(status.dot(), dot, "{label}");
        assert!(says(app, WINDOW, label), "the navigation says {label:?}");
        seen.push(label.to_owned());
    };

    // No server named.
    assert!(!app.collab.wanted());
    check(
        &app,
        SyncStatus::Off,
        "未连接协同服务器",
        ColorToken::TextMuted,
    );
    assert_eq!(chrome::server_address(&app), None);
    let at = drawn(&app, WINDOW);
    assert!(!at.contains_key("PID_CHROME:server"));
    assert!(
        !at.contains_key("PID_CHROME:avatar"),
        "nobody is named either"
    );

    // A server and a user named; the first connection is being made.
    app.collab.server_url = "ws://127.0.0.1:3240/ws".to_owned();
    app.collab.user_id = "wang".to_owned();
    check(
        &app,
        SyncStatus::Connecting,
        "正在连接协同服务器…",
        ColorToken::Amber,
    );
    let address = chrome::server_address(&app).expect("a server is named");
    assert!(address.contains("127.0.0.1:3240"), "{address}");
    assert!(
        says(&app, WINDOW, &address),
        "the title bar shows the address"
    );
    assert_eq!(chrome::user_initial(&app).as_deref(), Some("W"));
    let at = drawn(&app, WINDOW);
    assert!(at.contains_key("PID_CHROME:server"));
    assert!(at.contains_key("PID_CHROME:avatar"));

    // The connection is up.
    app.collab.connected = true;
    app.collab.base_revision = 12;
    check(
        &app,
        SyncStatus::Connected { revision: 12 },
        "已连接 · 修订 12",
        ColorToken::Success,
    );

    // The server has made this session read-only.
    app.collab.readonly = true;
    check(
        &app,
        SyncStatus::ReadOnly { revision: 12 },
        "已连接 · 只读 · 修订 12",
        ColorToken::Amber,
    );
    app.collab.readonly = false;

    // The drawing is no longer what the room has.
    app.collab
        .mark_diverged("op 7 could not be applied".to_owned());
    check(
        &app,
        SyncStatus::Diverged,
        "与房间不一致，已停止同步",
        ColorToken::Danger,
    );

    // The connection went down and has failed to come back three times.
    app.collab.connected = false;
    app.collab.reconnect_attempts = 3;
    check(
        &app,
        SyncStatus::Retrying { failed: 3 },
        "连接已断开，重试第 3 次",
        ColorToken::Danger,
    );

    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 6, "six states, six different texts");
}

/// The breadcrumb and the status bar say what is in front.
#[test]
fn the_bars_say_what_is_in_front() {
    let _one = crate::headless_ui_lock();
    let mut app = shell();
    let name = app.tabs[app.active_tab].tab_display_name();
    assert_eq!(
        chrome::breadcrumb(&app, PidScreen::Editor),
        format!("P&ID 编辑器 / {name}")
    );
    assert!(says(&app, WINDOW, &format!("P&ID 编辑器 / {name}")));

    app.snapper.snap_enabled = true;
    app.ortho_mode = false;
    assert_eq!(
        chrome::status_text(&app, PidScreen::Editor),
        "编辑中 · 捕捉 开 · 正交 关"
    );
    let _ = app.update(AppMessage::ToggleOrtho);
    let _ = app.update(AppMessage::ToggleSnapEnabled);
    assert_eq!(
        chrome::status_text(&app, PidScreen::Editor),
        "编辑中 · 捕捉 关 · 正交 开"
    );
    assert!(says(&app, WINDOW, "编辑中 · 捕捉 关 · 正交 开"));
    app.collab.readonly = true;
    assert_eq!(
        chrome::status_text(&app, PidScreen::Editor),
        "只读 · 捕捉 关 · 正交 开"
    );
}

// ── What can be pressed ──────────────────────────────────────────────────

/// Every part of the shell that takes a press does something; the parts
/// that only show something take none.
#[test]
fn every_part_of_the_shell_that_can_be_pressed_does_something() {
    let _one = crate::headless_ui_lock();
    let mut app = shell();
    app.collab.server_url = "ws://127.0.0.1:3240/ws".to_owned();
    app.collab.user_id = "wang".to_owned();
    let at = drawn(&app, WINDOW);

    // A navigation entry shows its screen.
    let entry = at[&chrome::nav_bounds_id(PidScreen::Editor)].center();
    let published = pointer(&mut app, WINDOW, &click(entry));
    assert_eq!(published, ["PidShell(Show(Editor))"]);
    assert_eq!(app.pid_shell.current(), Some(PidScreen::Editor));

    // Help opens the wiki (kept, in a test build, instead of opened).
    let _ = crate::sys::launches::taken();
    let published = pointer(&mut app, WINDOW, &click(at["PID_CHROME:help"].center()));
    assert_eq!(published, ["Command(\"HELP\")"]);
    assert_eq!(
        crate::sys::launches::taken(),
        ["open https://github.com/HakanSeven12/OpenCADStudio/wiki"]
    );

    // The rest shows and takes no press.
    for part in ["server", "crumb", "avatar", "sync"] {
        let place = at[&format!("PID_CHROME:{part}")].center();
        let published = pointer(&mut app, WINDOW, &click(place));
        assert!(published.is_empty(), "{part}: {published:?}");
    }
    // And nothing in the shell is left out of this test: these are all its
    // parts.
    let parts: Vec<&str> = at
        .keys()
        .filter_map(|id| id.strip_prefix(chrome::CHROME_BOUNDS_PREFIX))
        .collect();
    assert_eq!(
        parts,
        [
            "avatar",
            "crumb",
            "help",
            "nav",
            "nav:editor",
            "server",
            "status",
            "sync",
            "title"
        ]
    );
}

/// Drawing the whole window in a test — these tests do, to learn where
/// things are — leaves the process as it was: no application, in this test
/// or another, is told that nothing renders its scene and has the warning
/// dialog put in front of it.
#[test]
fn drawing_the_window_in_a_test_puts_no_gpu_warning_up() {
    let _one = crate::headless_ui_lock();
    let mut app = shell();
    for _ in 0..5 {
        let _ = drawn(&app, WINDOW);
    }
    let _ = app.update(AppMessage::Noop);
    assert!(app.active_modal.is_none());
    assert!(app.pending_startup_modals.is_empty());
    let _ = app.update(AppMessage::ToggleOrtho);
    assert!(app.ortho_mode, "a key still reaches the application");
}

// ── The general interface in its frame ───────────────────────────────────

/// The drawing area, in window coordinates.
fn canvas(app: &OpenCADStudio, window: Size) -> Rectangle {
    let _ = drawn(app, window);
    crate::ui::wrap_bar::window_bounds(crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID)
        .expect("the drawing area reports where it is")
}

/// The lines of the drawing in front, as `(start, end)`.
fn lines(app: &OpenCADStudio) -> Vec<([f64; 2], [f64; 2])> {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            document::EntityType::Line(line) => {
                Some(([line.start.x, line.start.y], [line.end.x, line.end.y]))
            }
            _ => None,
        })
        .collect()
}

/// The general interface is laid out in the shell's content area exactly
/// as in a window of that size, and the drawing area is where the frame
/// puts it.
#[test]
fn the_editor_is_laid_out_in_the_content_area_as_in_a_window_of_its_own() {
    let _one = crate::headless_ui_lock();
    let (inside, apart) = (shell(), alone());
    let in_shell = canvas(&inside, WINDOW);
    let on_its_own = canvas(&apart, CONTENT);
    assert_eq!(in_shell, on_its_own + ORIGIN);
    assert!(
        in_shell.width > 400.0 && in_shell.height > 300.0,
        "{in_shell:?}"
    );
}

/// The window after `steps`, as it is drawn then: RGBA, one pixel for each
/// logical pixel. The pointer stays where the last step left it, so what
/// hovering there shows is in the picture, and so is an overlay the steps
/// opened (a context menu, a tooltip).
fn picture(app: &mut OpenCADStudio, window: Size, steps: &[(Point, Pointer)]) -> Vec<u8> {
    use iced_test::core::renderer::Headless as _;
    use iced_test::core::{mouse, renderer, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut headless = headless();
    let mut cache = Cache::default();
    let mut cursor = mouse::Cursor::Unavailable;
    for (at, step) in steps {
        cursor = mouse::Cursor::Available(*at);
        let event = iced::Event::Mouse(match step {
            Pointer::Move => mouse::Event::CursorMoved { position: *at },
            Pointer::Press(button) => mouse::Event::ButtonPressed(*button),
            Pointer::Release(button) => mouse::Event::ButtonReleased(*button),
        });
        let mut messages = Vec::new();
        {
            let mut ui = UserInterface::build(app.view_main(), window, cache, &mut headless);
            let _ = ui.update(
                &core_window::Headless,
                &shell::Waker::noop(),
                std::slice::from_ref(&event),
                cursor,
                &mut headless,
                &mut messages,
            );
            cache = ui.into_cache();
        }
        for message in messages {
            let _ = app.update(message);
        }
    }
    let mut ui = UserInterface::build(app.view_main(), window, cache, &mut headless);
    // An interface lays its overlay out when it is updated, and draws none
    // it has not laid out.
    let _ = ui.update(
        &core_window::Headless,
        &shell::Waker::noop(),
        &[iced::Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))],
        cursor,
        &mut headless,
        &mut Vec::new(),
    );
    ui.draw(
        &mut headless,
        &app.active_theme,
        &renderer::Style {
            text_color: iced::Color::WHITE,
        },
        cursor,
    );
    headless.screenshot(
        Size::new(window.width as u32, window.height as u32),
        1.0,
        iced::Color::BLACK,
    )
}

/// The pixels of `area` out of a picture of a window `window` wide.
fn cut(pixels: &[u8], window: Size, area: Rectangle) -> Vec<u8> {
    let stride = window.width as usize * 4;
    let (left, top) = (area.x as usize, area.y as usize);
    let (width, height) = (area.width as usize, area.height as usize);
    let mut out = Vec::with_capacity(width * height * 4);
    for row in top..top + height {
        let start = row * stride + left * 4;
        out.extend_from_slice(&pixels[start..start + width * 4]);
    }
    out
}

/// How many pixels of two pictures of the same size differ by more than a
/// rounding of anti-aliasing, and the box they lie in: `(left, top, right,
/// bottom)`.
fn differing(a: &[u8], b: &[u8], width: usize) -> (usize, Option<(usize, usize, usize, usize)>) {
    assert_eq!(a.len(), b.len(), "pictures of one size");
    let mut count = 0;
    let mut within: Option<(usize, usize, usize, usize)> = None;
    for (index, (one, other)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        // An anti-aliased edge rasterized 208 pixels further right can come
        // out one level off in a channel (a rounded corner did); a thing in
        // another place differs by far more than that.
        let apart = one
            .iter()
            .zip(other)
            .any(|(one, other)| one.abs_diff(*other) > 2);
        if apart {
            count += 1;
            let (x, y) = (index % width, index / width);
            within = Some(match within {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
            });
        }
    }
    (count, within)
}

/// With `OCS_PID_SHELL_PICTURES` naming a folder, the two pictures a
/// comparison is about are written there, for looking at.
fn keep_pictures(what: &str, in_shell: &[u8], on_its_own: &[u8]) {
    let Some(folder) = std::env::var_os("OCS_PID_SHELL_PICTURES") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let name = what.replace(' ', "-");
    for (side, pixels) in [("in-shell", in_shell), ("on-its-own", on_its_own)] {
        if let Some(picture) =
            image::RgbaImage::from_raw(CONTENT.width as u32, CONTENT.height as u32, pixels.to_vec())
        {
            let _ = picture.save(folder.join(format!("{name}-{side}.png")));
        }
    }
}

/// Whatever the general interface shows — a hover, a dialog, a dropdown of
/// the ribbon, the Start page — it shows at the same place in the
/// shell's content area as in a window of its own: the two pictures are the
/// same, pixel for pixel (to within a level or two of anti-aliasing, see
/// `differing`). The drawing itself is not in them (a headless
/// frame has no GPU scene); where the pointer lands on it is the business
/// of the tests below.
#[test]
fn what_the_editor_shows_is_drawn_in_the_frame_where_a_window_of_its_own_draws_it() {
    let _one = crate::headless_ui_lock();
    let (mut inside, mut apart) = (shell(), alone());
    let frame = rect(ORIGIN.x, ORIGIN.y, CONTENT.width, CONTENT.height);
    let whole = rect(0.0, 0.0, CONTENT.width, CONTENT.height);
    // One frame each first: the corner widgets of the drawing area go by
    // what the frame before measured.
    let tab = drawn(&apart, CONTENT)["DOC_TAB:1"].center();
    assert_eq!(drawn(&inside, WINDOW)["DOC_TAB:1"].center(), tab + ORIGIN);

    let compare = |inside: &mut OpenCADStudio,
                   apart: &mut OpenCADStudio,
                   what: &str,
                   steps: &[(Point, Pointer)]| {
        let shifted: Vec<(Point, Pointer)> = steps
            .iter()
            .map(|(at, step)| (*at + ORIGIN, *step))
            .collect();
        let in_shell = cut(&picture(inside, WINDOW, &shifted), WINDOW, frame);
        let on_its_own = cut(&picture(apart, CONTENT, steps), CONTENT, whole);
        keep_pictures(what, &in_shell, &on_its_own);
        let (count, within) = differing(&in_shell, &on_its_own, CONTENT.width as usize);
        assert_eq!(count, 0, "{what}: {count} pixels differ, within {within:?}");
        on_its_own
    };
    // That a picture shows what it is said to show: it is not the picture
    // taken before the thing was brought up.
    let shows = |picture: &[u8], before: &[u8], what: &str| {
        let (count, _) = differing(picture, before, CONTENT.width as usize);
        assert!(
            count > 500,
            "{what} is in the picture ({count} pixels of it)"
        );
    };

    // As it starts.
    let plain = compare(&mut inside, &mut apart, "plain", &[]);
    // The pointer over a tool of the ribbon.
    compare(
        &mut inside,
        &mut apart,
        "hover",
        &[(Point::new(40.0, 70.0), Pointer::Move)],
    );
    // A dialog.
    for app in [&mut inside, &mut apart] {
        let _ = app.dispatch_command("DWGPROPS");
        assert!(app.active_modal.is_some(), "the dialog is up");
    }
    let dialog = compare(&mut inside, &mut apart, "dialog", &[]);
    shows(&dialog, &plain, "the dialog");
    for app in [&mut inside, &mut apart] {
        let _ = app.update(AppMessage::CloseModal);
        assert!(app.active_modal.is_none());
    }
    // A dropdown of the ribbon: the undo history, once there is a step.
    for app in [&mut inside, &mut apart] {
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 100,50"}"#);
        app.finish_all_pending_history();
        app.ribbon.open_dropdown = Some(crate::ui::ribbon::UNDO_HISTORY_ID.to_string());
    }
    let dropdown = compare(&mut inside, &mut apart, "dropdown", &[]);
    shows(&dropdown, &plain, "the dropdown");
    for app in [&mut inside, &mut apart] {
        app.ribbon.open_dropdown = None;
        // The Start page, which the general interface still has.
        let _ = app.update(AppMessage::TabSwitch(0));
        assert!(app.tabs[app.active_tab].is_start);
    }
    compare(&mut inside, &mut apart, "start page", &[]);
}

/// An overlay of something in a frame — the list that drops from a picker,
/// which is drawn above the window like a context menu or a tooltip — opens
/// where it opens in a window of its own, and takes the pointer there.
#[test]
fn an_overlay_of_what_is_in_a_frame_opens_where_it_would_in_a_window_of_its_own() {
    use iced::widget::{column, container, pick_list, row, Space};
    use iced::{Element, Fill};
    use iced_test::core::renderer::Headless as _;
    use iced_test::core::{mouse, renderer, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};

    let _one = crate::headless_ui_lock();
    const INNER: Size = Size::new(392.0, 356.0);
    const OUTER: Size = Size::new(INNER.width + ORIGIN.x, INNER.height + ORIGIN.y);
    fn picker<'a>() -> Element<'a, AppMessage> {
        container(
            pick_list(Some("alpha"), ["alpha", "beta", "gamma"], |name: &&str| {
                (*name).to_owned()
            })
            .on_select(|picked| AppMessage::Command(picked.to_owned()))
            .width(160),
        )
        .padding(40)
        .width(Fill)
        .height(Fill)
        .into()
    }
    fn framed<'a>() -> Element<'a, AppMessage> {
        column![
            Space::new().height(ORIGIN.y),
            row![Space::new().width(ORIGIN.x), rebase::Rebase::new(picker())],
        ]
        .into()
    }
    // The picture after `steps`, and what the widgets published.
    fn after(
        view: fn() -> Element<'static, AppMessage>,
        size: Size,
        steps: &[(Point, Pointer)],
    ) -> (Vec<u8>, Vec<String>) {
        let mut headless = headless();
        let mut cache = Cache::default();
        let mut cursor = mouse::Cursor::Unavailable;
        let mut published = Vec::new();
        for (at, step) in steps {
            cursor = mouse::Cursor::Available(*at);
            let event = iced::Event::Mouse(match step {
                Pointer::Move => mouse::Event::CursorMoved { position: *at },
                Pointer::Press(button) => mouse::Event::ButtonPressed(*button),
                Pointer::Release(button) => mouse::Event::ButtonReleased(*button),
            });
            let mut messages = Vec::new();
            let mut ui = UserInterface::build(view(), size, cache, &mut headless);
            let _ = ui.update(
                &core_window::Headless,
                &shell::Waker::noop(),
                std::slice::from_ref(&event),
                cursor,
                &mut headless,
                &mut messages,
            );
            cache = ui.into_cache();
            published.extend(messages.iter().map(|message| format!("{message:?}")));
        }
        let mut ui = UserInterface::build(view(), size, cache, &mut headless);
        // An interface lays its overlay out when it is updated, and draws
        // none it has not laid out.
        let _ = ui.update(
            &core_window::Headless,
            &shell::Waker::noop(),
            &[iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            ))],
            cursor,
            &mut headless,
            &mut Vec::new(),
        );
        ui.draw(
            &mut headless,
            &iced::Theme::Dark,
            &renderer::Style {
                text_color: iced::Color::WHITE,
            },
            cursor,
        );
        let pixels = headless.screenshot(
            Size::new(size.width as u32, size.height as u32),
            1.0,
            iced::Color::BLACK,
        );
        (pixels, published)
    }

    let frame = rect(ORIGIN.x, ORIGIN.y, INNER.width, INNER.height);
    let on_picker = Point::new(100.0, 55.0);
    // The second row of the open list, a row below the picker.
    let on_row = Point::new(100.0, 55.0 + 62.0);
    for (what, steps) in [
        ("closed", vec![]),
        ("open", click(on_picker).to_vec()),
        ("picked", [click(on_picker), click(on_row)].concat()),
    ] {
        let shifted: Vec<(Point, Pointer)> = steps
            .iter()
            .map(|(at, step)| (*at + ORIGIN, *step))
            .collect();
        let (alone, said_alone) = after(picker, INNER, &steps);
        let (in_frame, said_in_frame) = after(framed, OUTER, &shifted);
        let (count, within) =
            differing(&cut(&in_frame, OUTER, frame), &alone, INNER.width as usize);
        assert_eq!(count, 0, "{what}: {count} pixels differ, within {within:?}");
        assert_eq!(said_in_frame, said_alone, "{what}");
        if what == "open" {
            let (closed, _) = after(picker, INNER, &[]);
            let (list, _) = differing(&alone, &closed, INNER.width as usize);
            assert!(
                list > 500,
                "the open list is in the picture ({list} pixels)"
            );
        }
        if what == "picked" {
            assert_eq!(
                said_alone.len(),
                1,
                "a row of the list was picked: {said_alone:?}"
            );
        }
    }
}

/// Picking an object, selecting by a window and snapping to an end point
/// are the same with the shell around the editor as without, the pointer
/// being at the same place of the drawing area.
///
/// Two things are held. What the drawing area is told — every message its
/// widgets publish for a click on an object and for a window dragged round
/// it, with the positions they carry — is the same in both, which is all
/// that picking and selecting go by. (Whether the object ends up selected is
/// not asked here: a headless frame builds no pick geometry, and the count
/// is none in both.) And snapping is asked outright: a line begun a few
/// pixels off another's end starts on that end.
#[test]
fn selecting_and_snapping_in_the_shell_are_what_they_are_without_it() {
    let _one = crate::headless_ui_lock();
    let (mut inside, mut apart) = (shell(), alone());
    let area = canvas(&apart, CONTENT);
    let at = |fx: f32, fy: f32| Point::new(area.x + area.width * fx, area.y + area.height * fy);
    let left = iced::mouse::Button::Left;

    let mut results = Vec::new();
    for (app, window, shift) in [
        (&mut apart, CONTENT, iced::Vector::ZERO),
        (&mut inside, WINDOW, ORIGIN),
    ] {
        app.ortho_mode = false;
        // A line by two clicks, with nothing pulling the points.
        app.snapper.snap_enabled = false;
        let _ = app.dispatch_command("LINE");
        let _ = pointer(app, window, &click(at(0.3, 0.4) + shift));
        let _ = pointer(app, window, &click(at(0.7, 0.6) + shift));
        let _ = app.update(AppMessage::CommandEscape);
        app.finish_all_pending_history();

        // Picking it: a click on its middle.
        let pick = pointer(app, window, &click(at(0.5, 0.5) + shift));
        let _ = app.update(AppMessage::CommandEscape);

        // A window round it, corner to corner with the button held.
        let window_drag = pointer(
            app,
            window,
            &[
                (at(0.2, 0.3) + shift, Pointer::Move),
                (at(0.2, 0.3) + shift, Pointer::Press(left)),
                (at(0.5, 0.5) + shift, Pointer::Move),
                (at(0.8, 0.7) + shift, Pointer::Move),
                (at(0.8, 0.7) + shift, Pointer::Release(left)),
            ],
        );
        let _ = app.update(AppMessage::CommandEscape);

        // A second line begun a few pixels off the first one's end, with
        // object snap on.
        app.snapper.snap_enabled = true;
        let near = at(0.7, 0.6) + iced::Vector::new(3.0, 2.0);
        let _ = app.dispatch_command("LINE");
        let _ = pointer(app, window, &click(near + shift));
        let _ = pointer(app, window, &click(at(0.5, 0.8) + shift));
        let _ = app.update(AppMessage::CommandEscape);
        app.finish_all_pending_history();
        results.push((pick, window_drag, lines(app)));
    }
    assert_eq!(results[1], results[0], "in the shell, and on its own");
    let (pick, window_drag, drawn_lines) = &results[0];
    // The drawing area was told of the press, the moves and the release,
    // each with where it happened.
    assert!(pick.len() >= 3, "{pick:?}");
    assert!(window_drag.len() >= 4, "{window_drag:?}");
    assert!(
        window_drag
            .iter()
            .any(|message| message.contains("Move") && message.contains("Point")),
        "{window_drag:?}"
    );
    assert_eq!(drawn_lines.len(), 2, "{drawn_lines:?}");
    assert_eq!(
        drawn_lines[1].0, drawn_lines[0].1,
        "the second line starts on the first one's end: it snapped"
    );
}

/// A point picked on the canvas is the same point of the drawing with the
/// shell around the editor as without: a line drawn by two clicks.
#[test]
fn a_point_picked_in_the_shell_is_the_point_picked_without_it() {
    let _one = crate::headless_ui_lock();
    let (mut inside, mut apart) = (shell(), alone());
    let area = canvas(&apart, CONTENT);
    let from = Point::new(area.x + area.width * 0.3, area.y + area.height * 0.4);
    let to = Point::new(area.x + area.width * 0.7, area.y + area.height * 0.6);

    for (app, window, shift) in [
        (&mut apart, CONTENT, iced::Vector::ZERO),
        (&mut inside, WINDOW, ORIGIN),
    ] {
        // Neither aid may pull the picked point.
        app.snapper.snap_enabled = false;
        app.ortho_mode = false;
        let _ = app.dispatch_command("LINE");
        let _ = pointer(app, window, &click(from + shift));
        let _ = pointer(app, window, &click(to + shift));
        let _ = app.update(AppMessage::CommandEscape);
        app.finish_all_pending_history();
    }
    let (drawn_apart, drawn_inside) = (lines(&apart), lines(&inside));
    assert_eq!(drawn_apart.len(), 1, "{drawn_apart:?}");
    assert_eq!(drawn_inside, drawn_apart);
    let ([x0, y0], [x1, y1]) = drawn_apart[0];
    assert!(
        (x1 - x0).abs() > 1e-6 && (y1 - y0).abs() > 1e-6,
        "a real line"
    );
}
