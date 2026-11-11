//! The editor screen: its layout against the design, each thing that can be
//! pressed against the command it stands for, what the inspector shows
//! against the records, and what of the general interface is no longer in
//! the module.

use std::collections::BTreeMap;

use iced::{Point, Rectangle, Size};
use serde_json::{json, Value};

use super::editor::{self, Inspected, Tool};
use super::*;
use crate::cli::GuiConfig;
use crate::pid::persist::PidAuthor;
use crate::pid::topology::{ConnectorKind, TopologyTable};
use crate::ui::dock::PanelId;

/// The design's frame.
const WINDOW: Size = Size::new(1600.0, 1000.0);

// ── An application in a window ───────────────────────────────────────────

/// The application as a launch leaves it once the startup prompts have been
/// answered, in a window of the design's size: in the module, or with
/// `--classic` and a drawing.
fn launched(classic: bool) -> OpenCADStudio {
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
    let _ = app.update(AppMessage::WindowResized(WINDOW.width, WINDOW.height));
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

/// Draw the window once, what is open over it included; where every
/// reporting widget is, in window coordinates.
fn drawn(app: &OpenCADStudio) -> BTreeMap<String, Rectangle> {
    use iced_test::core::{mouse, renderer, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    crate::ui::wrap_bar::forget_bounds("");
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), WINDOW, Cache::default(), &mut renderer);
    // An interface lays what is open over it out when it is updated, and
    // draws none of it that it has not laid out.
    let _ = ui.update(
        &core_window::Headless,
        &shell::Waker::noop(),
        &[iced::Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut Vec::new(),
    );
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

/// Where the part of the editor called `name` is.
fn at(parts: &BTreeMap<String, Rectangle>, name: &str) -> Rectangle {
    *parts
        .get(&editor::part(name))
        .unwrap_or_else(|| panic!("{name} is drawn: {:?}", parts.keys().collect::<Vec<_>>()))
}

/// Whether the text `label` is in the window.
fn says(app: &OpenCADStudio, label: &str) -> bool {
    use iced_test::core::widget;
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    use iced_test::Selector;
    let mut renderer = headless();
    let mut ui = UserInterface::build(app.view_main(), WINDOW, Cache::default(), &mut renderer);
    let mut find = Selector::find(label);
    ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
    matches!(
        widget::Operation::finish(&find),
        widget::operation::Outcome::Some(Some(_))
    )
}

/// A click of the left button at `place`: what the widgets publish.
/// Nothing is given to the application.
fn press(app: &OpenCADStudio, place: Point) -> Vec<AppMessage> {
    use iced_test::core::{mouse, shell, window as core_window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};
    let mut renderer = headless();
    let mut cache = Cache::default();
    let mut published = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: place },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let mut ui = UserInterface::build(app.view_main(), WINDOW, cache, &mut renderer);
        let _ = ui.update(
            &core_window::Headless,
            &shell::Waker::noop(),
            &[iced::Event::Mouse(event)],
            mouse::Cursor::Available(place),
            &mut renderer,
            &mut published,
        );
        cache = ui.into_cache();
    }
    published
}

fn told(published: &[AppMessage]) -> Vec<String> {
    published
        .iter()
        .map(|message| format!("{message:?}"))
        .collect()
}

/// The same click, with what it publishes given to the application as the
/// runtime gives it. What was published, as text.
fn click(app: &mut OpenCADStudio, place: Point) -> Vec<String> {
    let published = press(app, place);
    let said = told(&published);
    for message in published {
        // Opening a drawing and bringing one forward finish in the task.
        let task = app.update(message);
        app.drive_headless_task(task)
            .expect("what the press started runs without a window");
    }
    said
}

/// One request on the control channel, as an automation client makes it.
fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let state = app.control_state();
    req["protocol"] = json!(1);
    req["document_id"] = state["document_id"].clone();
    req["revision"] = state["revision"].clone();
    req["client_id"] = json!("test");
    req["request_id"] = json!(format!(
        "pid-editor-{}",
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let (response, task) = app.control_request(req.clone());
    app.drive_headless_task(task).unwrap();
    if matches!(response["status"].as_str(), Some("accepted" | "running")) {
        app.control_request(json!({"op": "operation", "request_id": req["request_id"]}))
            .0
    } else {
        response
    }
}

/// Give the command that is running `points`, then Enter: what picks in
/// the drawing area and the Enter key end in. (Not through the automation
/// channel, which gives no input to a command the user started in the
/// window — and a tool pressed is the user starting one.)
fn feed(app: &mut OpenCADStudio, points: &[[f64; 3]]) {
    use crate::command::StepInput;
    for [x, y, z] in points {
        assert!(running(app).is_some(), "a command waits for the point");
        let point = glam::DVec3::new(*x, *y, *z);
        let i = app.active_tab;
        app.tabs[i].last_cursor_world = point;
        let task = app.feed_command(StepInput::Point(point));
        app.drive_headless_task(task)
            .expect("the point is taken without a window");
    }
    let task = app.feed_command(StepInput::Enter);
    app.drive_headless_task(task)
        .expect("Enter is taken without a window");
}

/// Type `line` on the command line.
fn run(app: &mut OpenCADStudio, line: &str) {
    let task = app.dispatch_command(line);
    app.drive_headless_task(task)
        .expect("the command starts without a window");
}

/// The name of the command that is running.
fn running(app: &OpenCADStudio) -> Option<&'static str> {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|command| command.name())
}

/// What the command line has said since entry `from`.
fn said_since(app: &OpenCADStudio, from: usize) -> Vec<String> {
    app.command_line.history[from..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect()
}

/// What is on the drawing in front: every entity as it is, with its P&ID
/// record, in the drawing's order; and the topology table.
fn drawing(app: &OpenCADStudio) -> (Vec<(String, Option<PidAuthor>)>, Value) {
    let document = &app.tabs[app.active_tab].scene.document;
    let entities = document
        .entities()
        .map(|entity| {
            (
                format!("{entity:?}"),
                PidAuthor::load(document, entity.common().handle),
            )
        })
        .collect();
    let topology = serde_json::to_value(TopologyTable::load(document)).expect("a table");
    (entities, topology)
}

/// The block reference placed last.
fn last_symbol(app: &OpenCADStudio) -> document::Handle {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            document::EntityType::Insert(insert) => Some(insert.common.handle),
            _ => None,
        })
        .last()
        .expect("a symbol was placed")
}

/// The drawing in front.
fn doc(app: &OpenCADStudio) -> &document::Document {
    &app.tabs[app.active_tab].scene.document
}

fn select(app: &mut OpenCADStudio, handle: document::Handle) {
    let _ = app.automation_op(r#"{"op":"select","clear":true}"#);
    let selected = request(
        app,
        json!({"op": "select", "handles": [format!("{:X}", handle.value())]}),
    );
    assert_eq!(selected["ok"], true, "{selected}");
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

// ── The layout ───────────────────────────────────────────────────────────

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

/// The toolbar, the palette and the inspector have the design's sizes —
/// read from the `.pen` — and the window is divided by them.
#[test]
fn the_parts_of_the_editor_have_the_sizes_the_design_gives_them() {
    let text = std::fs::read_to_string(PEN).unwrap_or_else(|e| panic!("{PEN}: {e}"));
    let pen: Value = serde_json::from_str(&text).expect("the design is JSON");
    let content = &pen["children"]
        .as_array()
        .expect("top-level nodes")
        .iter()
        .find(|node| node["id"] == "t4QrJ")
        .expect("the editor's frame")["descendants"]["tqvXC"];
    let designed = |name: &str, side: &str| {
        node_named(content, name).unwrap_or_else(|| panic!("no {name}"))[side]
            .as_f64()
            .unwrap_or_else(|| panic!("{name} has no {side}")) as f32
    };
    assert_eq!(editor::TOOLBAR_HEIGHT, designed("Toolbar", "height"));
    assert_eq!(editor::PALETTE_WIDTH, designed("Mini Palette", "width"));
    assert_eq!(editor::INSPECTOR_WIDTH, designed("Inspector", "width"));

    let _one = crate::headless_ui_lock();
    let app = launched(false);
    let parts = drawn(&app);
    assert_eq!(at(&parts, "toolbar"), rect(208.0, 44.0, 1392.0, 44.0));
    assert_eq!(at(&parts, "palette"), rect(208.0, 88.0, 56.0, 886.0));
    assert_eq!(at(&parts, "tabs"), rect(264.0, 88.0, 1036.0, 32.0));
    assert_eq!(at(&parts, "drawing"), rect(264.0, 120.0, 1036.0, 854.0));
    assert_eq!(at(&parts, "inspector"), rect(1300.0, 88.0, 300.0, 886.0));
    // What the drawing measures by is its frame.
    assert_eq!(app.win_size, (1036.0, 854.0));
    // And the frame is all drawing: nothing of the general interface takes
    // room above it, below it or beside it.
    let area = parts[crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID];
    assert_eq!(area, at(&parts, "drawing"));
}

// ── The general interface's bars ─────────────────────────────────────────

/// What the general interface reports of its ribbon, its quick access bar,
/// its drawing tabs and its status bar is in a `--classic` window and not
/// in the module; the Properties pallet is docked there and not here.
#[test]
fn the_general_interfaces_bars_are_not_in_the_module() {
    let _one = crate::headless_ui_lock();
    let general = launched(true);
    assert_eq!(general.pid_shell.current(), None);
    assert_eq!(general.win_size, (WINDOW.width, WINDOW.height));
    let in_general = drawn(&general);
    let module = launched(false);
    let in_module = drawn(&module);

    let bars: Vec<&String> = in_general
        .keys()
        .filter(|id| {
            *id == crate::ui::ribbon::APP_MENU_ID
                || id.starts_with("QAT_")
                || id.starts_with("DOC_TAB:")
        })
        .collect();
    assert!(
        bars.iter().any(|id| *id == crate::ui::ribbon::APP_MENU_ID)
            && bars.iter().any(|id| id.starts_with("QAT_"))
            && bars.iter().any(|id| id.starts_with("DOC_TAB:")),
        "the general interface has its menu button, its quick access bar and its tabs: {bars:?}"
    );
    // Its status bar: whatever it reports along the window's foot.
    let foot: Vec<&String> = in_general
        .iter()
        .filter(|(_, bounds)| bounds.y >= WINDOW.height - 40.0 && bounds.height <= 40.0)
        .map(|(id, _)| id)
        .collect();
    assert!(!foot.is_empty(), "the general status bar reports its parts");
    for id in bars.iter().chain(&foot) {
        assert!(!in_module.contains_key(*id), "{id} is not in the module");
    }
    // The ribbon took the top of the general window; the module's drawing
    // starts right under its own tabs.
    let area = crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID;
    assert!(in_general[area].y > 100.0, "{:?}", in_general[area]);
    assert!(
        in_general[area].y + in_general[area].height < WINDOW.height - 10.0,
        "the general status bar is under the drawing: {:?}",
        in_general[area]
    );

    // The Properties pallet, wanted in both: drawn in the general window,
    // and not beside the module's own inspector.
    let (mut general, mut module) = (general, module);
    general.show_properties = true;
    module.show_properties = true;
    assert!(general.dock_panel_visible(PanelId::Properties));
    assert!(!module.dock_panel_visible(PanelId::Properties));
    // Nothing of the module is in the general window.
    assert!(
        !in_general
            .keys()
            .any(|id| id.starts_with(editor::BOUNDS_PREFIX)
                || id.starts_with(chrome::CHROME_BOUNDS_PREFIX)
                || id.starts_with(BOUNDS_PREFIX)),
        "{:?}",
        in_general.keys().collect::<Vec<_>>()
    );
}

// ── The tools ────────────────────────────────────────────────────────────

/// Every tool the toolbar and the connector menu have.
fn tools(app: &OpenCADStudio) -> Vec<Tool> {
    let mut tools = vec![Tool::Pipe, Tool::Branch, Tool::Signal];
    tools.extend(
        editor::place_tools(app)
            .into_iter()
            .map(|(id, _)| Tool::Place(id)),
    );
    tools.extend(editor::CONNECTOR_KINDS.map(Tool::Connector));
    tools
}

/// Pressing a tool is typing its command: the same command is running
/// afterwards, and the command line has said the same.
#[test]
fn each_tool_starts_the_command_the_command_line_starts() {
    let _one = crate::headless_ui_lock();
    let lines: Vec<String> = tools(&launched(false))
        .iter()
        .map(|tool| tool.line().expect("a command"))
        .collect();
    assert_eq!(
        lines,
        [
            "PIDPIPE",
            "PIDPIPE run",
            "PIDPIPE signal",
            "PIDINSERT equipment",
            "PIDINSERT valve",
            "PIDINSERT instrument",
            "PIDCONNECTOR pipe_in",
            "PIDCONNECTOR pipe_out",
            "PIDCONNECTOR signal_in",
            "PIDCONNECTOR signal_out",
        ]
    );

    for tool in tools(&launched(false)) {
        let line = tool.line().expect("a command");
        let (mut pressed, mut typed) = (launched(false), launched(false));
        // A line for a run to go under and for a connector to sit on.
        for app in [&mut pressed, &mut typed] {
            run(app, "PIDPIPE");
            feed(app, &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
            run(app, "PIDPIPE signal");
            feed(app, &[[0.0, 100.0, 0.0], [200.0, 100.0, 0.0]]);
            assert_eq!(running(app), None);
        }
        let from = pressed.command_line.history.len();
        assert_eq!(typed.command_line.history.len(), from);

        if let Tool::Connector(_) = &tool {
            // The kinds are under the tool once it is pressed.
            let head = at(&drawn(&pressed), "tool:connector").center();
            assert_eq!(
                click(&mut pressed, head),
                ["PidShell(Editor(ConnectorMenu(true)))"]
            );
        }
        let button = at(&drawn(&pressed), &format!("tool:{}", tool.key())).center();
        let published = click(&mut pressed, button);
        assert_eq!(
            published,
            [format!("PidShell(Editor(Tool({tool:?})))")],
            "{line}"
        );
        let _ = typed.dispatch_command(&line);

        assert!(running(&pressed).is_some(), "{line} is running");
        assert_eq!(running(&pressed), running(&typed), "{line}");
        assert_eq!(
            said_since(&pressed, from),
            said_since(&typed, from),
            "{line}"
        );
        assert!(editor::tool_is_lit(&pressed, &tool), "{line} is lit");
        assert!(!editor::tool_is_lit(&pressed, &Tool::Select));
        assert!(!pressed.pid_shell.editor.connector_menu, "{line}");

        // 选择 ends it, as the Escape key does.
        let select = at(&drawn(&pressed), "tool:select").center();
        assert_eq!(
            click(&mut pressed, select),
            ["PidShell(Editor(Tool(Select)))"]
        );
        let _ = typed.update(AppMessage::CommandEscape);
        assert_eq!(running(&pressed), None);
        assert_eq!(
            said_since(&pressed, from),
            said_since(&typed, from),
            "{line}"
        );
        assert!(editor::tool_is_lit(&pressed, &Tool::Select));
        assert!(!editor::tool_is_lit(&pressed, &tool));
        assert_eq!(drawing(&pressed), drawing(&typed), "{line}");
    }
}

/// The rest of what the editor has to press, each against what it does.
#[test]
fn every_other_part_of_the_editor_that_can_be_pressed_does_what_it_says() {
    let _one = crate::headless_ui_lock();
    let mut app = launched(false);
    let parts = drawn(&app);

    // 保存 is QSAVE (not run here: an unnamed drawing asks for a file).
    assert_eq!(
        told(&press(&app, at(&parts, "save").center())),
        ["Command(\"QSAVE\")"]
    );
    // The three steps of the zoom control.
    for (step, line) in [
        ("zoom:out", "ZOOM OUT"),
        ("zoom:in", "ZOOM IN"),
        ("zoom:fit", "ZOOM EXTENTS"),
    ] {
        assert_eq!(
            told(&press(&app, at(&parts, step).center())),
            [format!("Command({line:?})")]
        );
    }
    // 打开 asks for a file (not run here); 新建 opens a blank drawing.
    assert_eq!(
        told(&press(&app, at(&parts, "open").center())),
        ["OpenFile"]
    );
    let first = app.tabs[app.active_tab].id;
    assert_eq!(click(&mut app, at(&parts, "new").center()), ["TabNew"]);
    let second = app.tabs[app.active_tab].id;
    assert_ne!(second, first, "the new drawing is in front");

    // A tab brings its drawing forward; its cross closes it.
    let parts = drawn(&app);
    let index = app
        .tabs
        .iter()
        .position(|tab| tab.id == first)
        .expect("the first drawing");
    assert_eq!(
        click(&mut app, at(&parts, &format!("tab:{first}")).center()),
        [format!("TabSwitch({index})")]
    );
    assert_eq!(app.tabs[app.active_tab].id, first);
    assert_eq!(
        told(&press(
            &app,
            at(&parts, &format!("tab-close:{second}")).center()
        )),
        [format!("TabClose({second})")]
    );
    // The general interface's Start tab is not one of them.
    assert!(app.tabs.iter().any(|tab| tab.is_start));
    let tabs: Vec<&String> = parts
        .keys()
        .filter(|id| id.starts_with(&editor::part("tab:")))
        .collect();
    assert_eq!(tabs.len(), 2, "{tabs:?}");

    // The switch of the command line.
    assert!(!app.command_line_hidden);
    assert_eq!(
        click(&mut app, at(&parts, "command-line").center()),
        ["Command(\"COMMANDLINETOGGLE\")"]
    );
    assert!(app.command_line_hidden);
    let switch = at(&drawn(&app), "command-line").center();
    let _ = click(&mut app, switch);
    assert!(!app.command_line_hidden);

    // A square of the palette starts placing its symbol, as the docked
    // palette's cell does (what is placed is compared with the command
    // line's in the test of the line, the symbol and the connector).
    let block = "PID_SYM_equipment_pump";
    let square = at(&drawn(&app), &format!("symbol:{block}")).center();
    assert_eq!(
        click(&mut app, square),
        [format!("PidShell(Editor(Symbol({block:?})))")]
    );
    assert_eq!(running(&app), Some("PIDINSERT"));
    assert!(editor::symbol_is_lit(&app, block));
    assert!(!editor::symbol_is_lit(&app, "PID_SYM_equipment_vessel"));
    // Every symbol of the library has its square, and no other does.
    let squares: Vec<String> = drawn(&app)
        .keys()
        .filter_map(|id| id.strip_prefix(&editor::part("symbol:")).map(str::to_owned))
        .collect();
    let mut library = app.pid_symbols.block_names();
    library.sort();
    assert_eq!(squares, library);
}

// ── A line, a symbol, a connector ────────────────────────────────────────

/// A pipe line, a symbol and a connector made from the editor — the tool,
/// the palette's square and the connector menu pressed — are, object for
/// object and record for record, what the same three typed on the command
/// line are.
#[test]
fn a_line_a_symbol_and_a_connector_from_the_editor_are_what_the_command_line_makes() {
    let _one = crate::headless_ui_lock();
    let (mut pressed, mut typed) = (launched(false), launched(false));

    // A pipe line.
    let tool = at(&drawn(&pressed), "tool:pipe").center();
    let _ = click(&mut pressed, tool);
    run(&mut typed, "PIDPIPE");
    for app in [&mut pressed, &mut typed] {
        feed(app, &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
    }
    // A symbol.
    let block = "PID_SYM_equipment_vessel";
    let square = at(&drawn(&pressed), &format!("symbol:{block}")).center();
    let _ = click(&mut pressed, square);
    run(&mut typed, &format!("PIDINSERT {block}"));
    for app in [&mut pressed, &mut typed] {
        feed(app, &[[80.0, 150.0, 0.0]]);
    }
    // A connector on the line.
    let head = at(&drawn(&pressed), "tool:connector").center();
    let _ = click(&mut pressed, head);
    let kind = Tool::Connector(ConnectorKind::PipeOut);
    let item = at(&drawn(&pressed), &format!("tool:{}", kind.key())).center();
    assert_eq!(
        click(&mut pressed, item),
        [format!("PidShell(Editor(Tool({kind:?})))")]
    );
    run(&mut typed, "PIDCONNECTOR pipe_out");
    for app in [&mut pressed, &mut typed] {
        feed(app, &[[80.0, 0.0, 0.0]]);
        assert_eq!(running(app), None);
    }

    let (entities, topology) = drawing(&pressed);
    assert_eq!((entities.clone(), topology.clone()), drawing(&typed));
    // And the three are there.
    let types: Vec<&str> = entities
        .iter()
        .filter_map(|(_, author)| author.as_ref().map(|author| author.type_code.as_str()))
        .collect();
    assert!(types.contains(&"PIPE_LINE"), "{types:?}");
    assert!(types.contains(&"EQUIPMENT"), "{types:?}");
    assert_eq!(topology["pipe_lines"].as_array().map(Vec::len), Some(1));
    assert_eq!(topology["connectors"].as_array().map(Vec::len), Some(1));
    assert_eq!(topology["connectors"][0]["kind"], "pipe_out");
}

// ── The inspector ────────────────────────────────────────────────────────

/// The inspector shows the selected object as its records have it, and a
/// value typed in it lands in the record — the same record `PIDSETPROP`
/// leaves.
#[test]
fn the_inspector_shows_the_selected_objects_record_and_writes_to_it() {
    let _one = crate::headless_ui_lock();
    let (mut edited, mut typed) = (launched(false), launched(false));
    assert_eq!(editor::inspected(&edited), Inspected::Nothing);
    assert!(says(&edited, "没有选中对象"));

    for app in [&mut edited, &mut typed] {
        run(app, "PIDINSERT vessel");
        feed(app, &[[80.0, 150.0, 0.0]]);
        let handle = last_symbol(app);
        select(app, handle);
    }
    let handle = last_symbol(&edited);
    let author = PidAuthor::load(doc(&edited), handle).expect("a record");
    let Inspected::Object(object) = editor::inspected(&edited) else {
        panic!("one P&ID object is selected");
    };
    assert_eq!(object.handle, handle);
    assert_eq!(object.tag, author.tag);
    assert_eq!(object.type_code, author.type_code);
    assert_eq!(object.symbol.as_deref(), Some("容器"));
    let Some(document::EntityType::Insert(insert)) = doc(&edited).get_entity(handle) else {
        panic!("the symbol is a block reference");
    };
    assert_eq!(
        object.placement,
        Some((insert.x_scale(), insert.rotation.to_degrees()))
    );
    assert_eq!(object.ports, None, "not a fitting");
    // The fields of its kind, each with what the record has: nothing yet.
    let keys: Vec<&str> = object.fields.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["名称", "描述", "设计压力", "设计温度", "操作压力"]);
    assert!(object.fields.iter().all(|(_, value)| value.is_empty()));
    // It is on the screen: the tag, the type and each field's row.
    assert!(!author.tag.is_empty() && says(&edited, &author.tag));
    assert!(says(&edited, "EQUIPMENT / 容器"));
    let parts = drawn(&edited);
    for row in ["pid_tag", "pid_props_名称", "pid_props_设计压力"] {
        let _ = at(&parts, &format!("field:{row}"));
    }

    // A value typed into a row and entered.
    let send = |app: &mut OpenCADStudio, message: editor::Message| {
        let _ = app.update(AppMessage::PidShell(Message::Editor(message)));
    };
    send(
        &mut edited,
        editor::Message::FieldInput {
            handle,
            field: "pid_props_设计压力".to_owned(),
            text: "1.6MPa".to_owned(),
        },
    );
    // Typed, not entered: the record is as it was.
    assert_eq!(
        PidAuthor::load(doc(&edited), handle).expect("a record"),
        author
    );
    send(&mut edited, editor::Message::FieldSubmit);
    let _ = typed.dispatch_command("PIDSETPROP 设计压力 1.6MPa");
    let after = PidAuthor::load(doc(&edited), handle).expect("a record");
    assert_ne!(after, author);
    assert_eq!(
        after,
        PidAuthor::load(doc(&typed), last_symbol(&typed)).expect("a record"),
        "the record PIDSETPROP leaves"
    );
    let Inspected::Object(object) = editor::inspected(&edited) else {
        panic!("still selected");
    };
    assert!(
        object
            .fields
            .contains(&("设计压力".to_owned(), "1.6MPa".to_owned())),
        "{:?}",
        object.fields
    );

    // The tag.
    send(
        &mut edited,
        editor::Message::FieldInput {
            handle,
            field: "pid_tag".to_owned(),
            text: "V-9001".to_owned(),
        },
    );
    send(&mut edited, editor::Message::FieldSubmit);
    let retagged = PidAuthor::load(doc(&edited), handle).expect("a record");
    assert_eq!(retagged.tag, "V-9001");
    assert_eq!(retagged.tag_source, "manual");
    assert!(matches!(
        editor::inspected(&edited),
        Inspected::Object(object) if object.tag == "V-9001"
    ));

    // A session that may not write: the row takes nothing, and says so.
    edited.collab.readonly = true;
    let before = edited.command_line.history.len();
    send(
        &mut edited,
        editor::Message::FieldInput {
            handle,
            field: "pid_tag".to_owned(),
            text: "V-9002".to_owned(),
        },
    );
    send(&mut edited, editor::Message::FieldSubmit);
    assert_eq!(
        PidAuthor::load(doc(&edited), handle).expect("a record"),
        retagged
    );
    assert!(
        !said_since(&edited, before).is_empty(),
        "the refusal is said"
    );
    assert!(
        !drawn(&edited).contains_key(&editor::part("field:pid_tag")),
        "and the rows are not typed in"
    );
}

/// What the inspector says of a pipe line, of an object that is not a P&ID
/// object, and of several.
#[test]
fn the_inspector_says_what_kind_of_thing_is_selected() {
    let mut app = launched(false);
    run(&mut app, "PIDPIPE");
    feed(&mut app, &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
    run(&mut app, "LINE");
    feed(&mut app, &[[0.0, 50.0, 0.0], [10.0, 60.0, 0.0]]);
    let handles: Vec<(document::Handle, bool)> = app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .map(|entity| {
            let handle = entity.common().handle;
            let document = &app.tabs[app.active_tab].scene.document;
            (handle, PidAuthor::load(document, handle).is_some())
        })
        .collect();
    let plain = handles
        .iter()
        .find(|(_, authored)| !authored)
        .expect("the plain line")
        .0;
    select(&mut app, plain);
    assert!(
        matches!(editor::inspected(&app), Inspected::Plain(kind) if !kind.is_empty()),
        "{:?}",
        editor::inspected(&app)
    );

    let line = handles
        .iter()
        .map(|(handle, _)| *handle)
        .find(|handle| {
            let document = &app.tabs[app.active_tab].scene.document;
            PidAuthor::load(document, *handle).is_some_and(|author| author.type_code == "PIPE_LINE")
        })
        .expect("the pipe line");
    select(&mut app, line);
    let Inspected::Object(object) = editor::inspected(&app) else {
        panic!("the pipe line is a P&ID object");
    };
    let keys: Vec<&str> = object.fields.iter().map(|(key, _)| key.as_str()).collect();
    assert!(
        crate::pid::propagate::LINE_FIELDS
            .iter()
            .all(|field| keys.contains(field)),
        "a line has the five line fields: {keys:?}"
    );
    assert_eq!(object.placement, None, "not a block reference");

    let _ = app.automation_op(r#"{"op":"select","clear":true}"#);
    let both = request(
        &mut app,
        json!({"op": "select", "handles": [
            format!("{:X}", plain.value()),
            format!("{:X}", line.value()),
        ]}),
    );
    assert_eq!(both["ok"], true, "{both}");
    assert_eq!(editor::inspected(&app), Inspected::Several(2));
}

// ── Which tool is lit, and what a row holds ──────────────────────────────

/// The tools that are lit, of all the toolbar and the connector menu have.
fn lit(app: &OpenCADStudio) -> Vec<Tool> {
    let mut all = vec![Tool::Select];
    all.extend(tools(app));
    all.into_iter()
        .filter(|tool| editor::tool_is_lit(app, tool))
        .collect()
}

/// The squares of the palette that are lit, by block name.
fn lit_squares(app: &OpenCADStudio) -> Vec<String> {
    app.pid_symbols
        .block_names()
        .into_iter()
        .filter(|block| editor::symbol_is_lit(app, block))
        .collect()
}

/// Which tool is lit is read off the command that is running — its name
/// and what it was started with — and off nothing else: it is the same
/// whether the command was typed or a tool was pressed, and whatever was
/// pressed before.
#[test]
fn the_tool_that_is_lit_is_the_command_that_is_running() {
    let mut app = launched(false);
    // A line for a run to go under and for a connector to sit on.
    run(&mut app, "PIDPIPE");
    feed(&mut app, &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
    run(&mut app, "PIDPIPE signal");
    feed(&mut app, &[[0.0, 100.0, 0.0], [200.0, 100.0, 0.0]]);
    assert_eq!(lit(&app), [Tool::Select], "nothing is running");
    assert!(lit_squares(&app).is_empty());

    // Typed, never pressed: each command lights its own tool and no other.
    for tool in tools(&app) {
        let line = tool.line().expect("a command");
        run(&mut app, &line);
        assert_eq!(lit(&app), [tool.clone()], "{line}, typed");
        let _ = app.update(AppMessage::CommandEscape);
        assert_eq!(lit(&app), [Tool::Select], "{line}, ended");
    }
    // With an argument the tool does not give: still that kind of command.
    run(&mut app, "PIDPIPE run L-1001");
    assert_eq!(lit(&app), [Tool::Branch]);
    let _ = app.update(AppMessage::CommandEscape);

    // What was pressed before does not count: 管线 pressed and ended, then
    // a signal line typed.
    let press = |app: &mut OpenCADStudio, message: editor::Message| {
        let _ = app.update(AppMessage::PidShell(Message::Editor(message)));
    };
    press(&mut app, editor::Message::Tool(Tool::Pipe));
    assert_eq!(lit(&app), [Tool::Pipe]);
    let _ = app.update(AppMessage::CommandEscape);
    run(&mut app, "PIDPIPE signal");
    assert_eq!(lit(&app), [Tool::Signal], "typed after 管线 was pressed");
    let _ = app.update(AppMessage::CommandEscape);

    // A symbol being placed lights its square and its category's tool,
    // typed by its id, typed by its block name or pressed.
    let pump = "PID_SYM_equipment_pump".to_owned();
    let vessel = "PID_SYM_equipment_vessel".to_owned();
    run(&mut app, "PIDINSERT pump");
    assert_eq!(lit_squares(&app), [pump.clone()]);
    assert_eq!(lit(&app), [Tool::Place("equipment".to_owned())]);
    let _ = app.update(AppMessage::CommandEscape);
    press(&mut app, editor::Message::Symbol(pump.clone()));
    assert_eq!(lit_squares(&app), [pump.clone()]);
    let _ = app.update(AppMessage::CommandEscape);
    run(&mut app, &format!("PIDINSERT {vessel}"));
    assert_eq!(
        lit_squares(&app),
        [vessel],
        "typed after the pump was pressed"
    );
    assert_eq!(lit(&app), [Tool::Place("equipment".to_owned())]);
    let _ = app.update(AppMessage::CommandEscape);
    run(&mut app, "PIDINSERT gate_valve");
    assert_eq!(lit(&app), [Tool::Place("valve".to_owned())]);
    let _ = app.update(AppMessage::CommandEscape);

    // A command no tool stands for lights none, 选择 included.
    run(&mut app, "LINE");
    assert!(lit(&app).is_empty(), "{:?}", lit(&app));
    assert!(lit_squares(&app).is_empty());
}

/// A row that was typed in and not entered holds what was typed only while
/// its object stays the one selected. Select another object and come back:
/// the row is the object's value again, and Enter there writes nothing.
#[test]
fn a_row_typed_in_is_dropped_when_another_object_is_selected() {
    let mut app = launched(false);
    run(&mut app, "PIDINSERT vessel");
    feed(&mut app, &[[80.0, 150.0, 0.0]]);
    let first = last_symbol(&app);
    run(&mut app, "PIDINSERT pump");
    feed(&mut app, &[[200.0, 150.0, 0.0]]);
    let second = last_symbol(&app);
    assert_ne!(first, second);
    let record = |app: &OpenCADStudio, handle| PidAuthor::load(doc(app), handle).expect("a record");
    let (before_first, before_second) = (record(&app, first), record(&app, second));
    let send = |app: &mut OpenCADStudio, message: editor::Message| {
        let _ = app.update(AppMessage::PidShell(Message::Editor(message)));
    };

    select(&mut app, first);
    send(
        &mut app,
        editor::Message::FieldInput {
            handle: first,
            field: "pid_props_设计压力".to_owned(),
            text: "1.6MPa".to_owned(),
        },
    );
    // Typed, not entered; then another object, and back.
    select(&mut app, second);
    assert!(
        matches!(editor::inspected(&app), Inspected::Object(object) if object.handle == second)
    );
    select(&mut app, first);
    send(&mut app, editor::Message::FieldSubmit);
    assert_eq!(
        record(&app, first),
        before_first,
        "what was typed before the other object was selected is gone"
    );
    assert_eq!(record(&app, second), before_second);

    // While the object stays selected the row keeps what is typed, and
    // Enter writes it.
    send(
        &mut app,
        editor::Message::FieldInput {
            handle: first,
            field: "pid_props_设计压力".to_owned(),
            text: "2.5MPa".to_owned(),
        },
    );
    send(&mut app, editor::Message::FieldSubmit);
    let Inspected::Object(object) = editor::inspected(&app) else {
        panic!("the first symbol is selected");
    };
    assert!(
        object
            .fields
            .contains(&("设计压力".to_owned(), "2.5MPa".to_owned())),
        "{:?}",
        object.fields
    );
}

// ── While a dialog is up, and without a drawing ──────────────────────────

/// While the general interface has a dialog up in the drawing, nothing of
/// the module round it takes a press: not the navigation, not a tool, not
/// the palette, not a tab, not a switch of the status bar.
#[test]
fn nothing_round_the_drawing_takes_a_press_while_a_dialog_is_up() {
    let _one = crate::headless_ui_lock();
    let mut app = launched(false);
    let parts = drawn(&app);
    let places = [
        parts[&chrome::nav_bounds_id(PidScreen::Editor)].center(),
        parts["PID_CHROME:help"].center(),
        parts["PID_CHROME:status:ortho"].center(),
        at(&parts, "tool:pipe").center(),
        at(&parts, "symbol:PID_SYM_equipment_vessel").center(),
        at(&parts, "new").center(),
        at(&parts, "save").center(),
    ];
    for place in places {
        assert_eq!(press(&app, place).len(), 1, "{place:?} takes a press");
    }

    let _ = app.dispatch_command("DWGPROPS");
    assert!(app.active_modal.is_some(), "the dialog is up");
    assert!(app.dialog_is_up());
    for place in places {
        let published = press(&app, place);
        assert!(published.is_empty(), "{place:?}: {:?}", told(&published));
    }

    let _ = app.update(AppMessage::CloseModal);
    assert!(!app.dialog_is_up());
    for place in places {
        assert_eq!(press(&app, place).len(), 1, "{place:?} takes a press again");
    }
}

/// With no drawing open the editor says so where the drawing would be, the
/// tools take no press, and a drawing can be made from there. The general
/// interface's Start page is not what stands there.
#[test]
fn without_a_drawing_the_editor_says_so_and_offers_one() {
    let _one = crate::headless_ui_lock();
    let mut app = launched(false);
    let only = app.tabs[app.active_tab].id;
    let closing = app.update(AppMessage::TabClose(only));
    app.drive_headless_task(closing)
        .expect("the drawing closes without a window");
    assert!(app.tabs[app.active_tab].is_start, "no drawing is open");
    assert!(!editor::has_drawing(&app));

    assert!(says(&app, "没有打开的图纸"));
    let parts = drawn(&app);
    for tool in [
        "tool:pipe",
        "tool:select",
        "save",
        "symbol:PID_SYM_equipment_vessel",
    ] {
        let published = press(&app, at(&parts, tool).center());
        assert!(published.is_empty(), "{tool}: {:?}", told(&published));
    }
    assert!(!parts.contains_key(&editor::part("zoom:in")));
    assert!(!parts.contains_key(&editor::part("command-line")));
    assert_eq!(editor::inspected(&app), Inspected::Nothing);

    assert_eq!(
        click(&mut app, at(&parts, "empty-new").center()),
        ["TabNew"]
    );
    assert!(editor::has_drawing(&app), "a drawing is in front");
    assert!(!says(&app, "没有打开的图纸"));
}

/// A session that may not write to the drawing has no tool to press but
/// 选择, and the inspector's rows are not typed in.
#[test]
fn a_read_only_session_has_no_writing_tool() {
    let _one = crate::headless_ui_lock();
    let mut app = launched(false);
    app.collab.readonly = true;
    let parts = drawn(&app);
    for tool in [
        "tool:pipe",
        "tool:branch",
        "tool:signal",
        "tool:connector",
        "save",
        "symbol:PID_SYM_equipment_vessel",
    ] {
        let published = press(&app, at(&parts, tool).center());
        assert!(published.is_empty(), "{tool}: {:?}", told(&published));
    }
    assert_eq!(
        told(&press(&app, at(&parts, "tool:select").center())),
        ["PidShell(Editor(Tool(Select)))"]
    );
}
