//! 移动管件后的管段自动补位 —— **判定与落表已下沉到 scene 层**。
//!
//! 实现在 [`crate::scene::pid_refill`]，由
//! [`crate::scene::Scene::transform_entities_checked`] 在 `Translate` 分支里
//! 调用。下沉的理由：真实产品的移动路径有三条 —— GUI 拖动 grip
//! （`commit_active_grip_edit`）、`MOVE` 命令（`handle_transform_selected`）、
//! `grip_drag` op（同一个 grip 提交）—— 它们各自直接改实体。只把补位挂在
//! REST 控制面上的实现只能证明验证通道可用，产品路径上不可达（M5 scrutiny
//! round 1 的 blocking）。
//!
//! 本模块保留原有 7 条端到端单测（走 `entities_transform` 控制 op），它们现在
//! 验证的是 scene 层的新实现；`VAL-PIPE-019..022` 的判据不变。

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use crate::pid::topology::{handle_text, parse_handle, TopologyTable, VALVE_PORT_NAMES};
    use crate::scene::pid_refill::REFILL_OUT_OF_RANGE;
    use serde_json::{json, Value};

    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        if let Some(object) = req.as_object_mut() {
            object.insert("document_id".into(), json!(app.tabs[app.active_tab].id));
            if !object.contains_key("request_id") {
                let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                object.insert("request_id".into(), json!(format!("pidmove-test-{serial}")));
            }
        }
        app.control_request(req).0
    }

    fn draw(app: &mut OpenCADStudio, cmd: &str, points: &[[f64; 3]]) -> Value {
        let started = request(app, json!({"op":"run","cmd":cmd}));
        assert_eq!(started["ok"], true, "{cmd} 应启动：{started}");
        for point in points {
            let fed = request(
                app,
                json!({"op":"input","kind":"point","point":point,"space":"wcs"}),
            );
            assert_eq!(fed["ok"], true, "喂点应被接受：{fed}");
        }
        request(app, json!({"op":"input","kind":"enter"}))
    }

    fn topology(app: &OpenCADStudio) -> TopologyTable {
        TopologyTable::load(&app.tabs[app.active_tab].scene.document)
    }

    fn hex(handle: codec::Handle) -> String {
        handle_text(handle)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6
    }

    /// 屏幕像素投影会带约 5e-6 的取整误差，拖动类断言用它。
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-3
    }

    /// 一条 200 单位长的管路，中点上插一个闸阀。返回（阀门 handle, 段 A, 段 B）。
    fn line_with_valve_at_midpoint(
        app: &mut OpenCADStudio,
    ) -> (codec::Handle, codec::Handle, codec::Handle) {
        draw(app, "PIDPIPE", &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
        draw(app, "PIDPIPE run", &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
        let done = draw(app, "PIDFITTING gate_valve", &[[100.0, 0.0, 0.0]]);
        assert_eq!(done["ok"], true, "{done}");
        let table = topology(app);
        assert_eq!(table.pipe_segments.len(), 2, "中点插阀门应拆成两段");
        assert_eq!(table.fittings.len(), 1);
        let valve = parse_handle(&table.fittings[0].handle).expect("阀门 handle");
        let a = parse_handle(&table.pipe_segments[0].handle).expect("段 A");
        let b = parse_handle(&table.pipe_segments[1].handle).expect("段 B");
        (valve, a, b)
    }

    fn move_valve(app: &mut OpenCADStudio, valve: codec::Handle, dx: f64) -> Value {
        request(
            app,
            json!({
                "op":"entities_transform",
                "action":"move",
                "handles":[hex(valve)],
                "vector":[dx,0,0],
            }),
        )
    }

    fn insert_point(app: &OpenCADStudio, handle: codec::Handle) -> [f64; 3] {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Insert(insert)) => [
                insert.insert_point.x,
                insert.insert_point.y,
                insert.insert_point.z,
            ],
            other => panic!("不是 Insert：{other:?}"),
        }
    }

    /// `VAL-PIPE-019`：平移 +40 后阀门到 140，两侧管段内侧端点等于阀门端口坐标。
    #[test]
    fn moving_the_valve_drags_the_inner_endpoints_onto_its_ports() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        assert!(close(insert_point(&app, valve)[0], 100.0), "阀门起初在 100");

        let moved = move_valve(&mut app, valve, 40.0);
        assert_eq!(moved["ok"], true, "{moved}");
        assert!(close(insert_point(&app, valve)[0], 140.0), "位移必须真的生效");

        let table = topology(&app);
        let a = &table.pipe_segments[0];
        let b = &table.pipe_segments[1];
        // 闸阀端口在 ±14：内侧端点落在 126 与 154。
        assert!(close(a.end.point[0], 126.0), "段 A 内侧端点：{:?}", a.end.point);
        assert!(close(b.start.point[0], 154.0), "段 B 内侧端点：{:?}", b.start.point);
        // 与阀门端口坐标逐个相等。
        let ports = placed_ports(&app, valve);
        assert_eq!(ports.len(), 2, "阀门两个端口：{ports:?}");
        assert!(close(a.end.point[0], ports[0].0) && close(a.end.point[1], ports[0].1));
        assert!(close(b.start.point[0], ports[1].0) && close(b.start.point[1], ports[1].1));
    }

    fn placed_ports(app: &OpenCADStudio, valve: codec::Handle) -> Vec<(f64, f64)> {
        let document = &app.tabs[app.active_tab].scene.document;
        match document.get_entity(valve) {
            Some(codec::EntityType::Insert(insert)) => {
                crate::pid::symbols::placed_ports(document, insert)
            }
            _ => Vec::new(),
        }
    }

    /// `VAL-PIPE-020`：外侧端点不动，两段沿 X 不重叠，长度守恒。
    #[test]
    fn the_refilled_segments_stay_joined_without_overlap_or_gap() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        let moved = move_valve(&mut app, valve, 40.0);
        assert_eq!(moved["ok"], true, "{moved}");

        let table = topology(&app);
        let a = &table.pipe_segments[0];
        let b = &table.pipe_segments[1];
        assert!(close(a.start.point[0], 0.0) && close(a.start.point[1], 0.0), "段 A 起点不动");
        assert!(close(b.end.point[0], 200.0) && close(b.end.point[1], 0.0), "段 B 终点不动");
        assert!(a.end.point[0] < b.start.point[0], "两段沿 X 严格不重叠");

        // 长度守恒：段 A + 阀门外形 + 段 B == 200。
        let ports = placed_ports(&app, valve);
        let (v0, v1) = (ports[0].0, ports[1].0);
        let total =
            (a.end.point[0] - a.start.point[0]) + (b.end.point[0] - b.start.point[0]) + (v1 - v0);
        assert!((total - 200.0).abs() <= 1e-6, "长度守恒偏差：{total}");
        // 兜底判据（实现不暴露 v0/v1 时用）。
        assert!(a.end.point[0] <= 140.0 && 140.0 <= b.start.point[0], "{a:?} {b:?}");
    }

    /// `VAL-PIPE-021`：移动不改变管段数与端口绑定，阀门 handle 与位号不变。
    #[test]
    fn moving_the_valve_keeps_the_segment_count_and_port_bindings() {
        let mut app = app();
        let (valve, seg_a, seg_b) = line_with_valve_at_midpoint(&mut app);
        let before = topology(&app);
        assert_eq!(before.pipe_runs[0].segments.len(), 2);
        let ports_before = before.fittings[0].ports.clone();

        let moved = move_valve(&mut app, valve, 40.0);
        assert_eq!(moved["ok"], true, "{moved}");

        let after = topology(&app);
        assert_eq!(after.pipe_runs[0].segments.len(), 2, "管段数不变");
        assert_eq!(after.pipe_segments.len(), 2, "不重建管段");
        assert_eq!(after.fittings.len(), 1);
        assert_eq!(after.fittings[0].ports, ports_before, "端口绑定不变");
        assert_eq!(
            after.fittings[0].ports[VALVE_PORT_NAMES[0]].as_deref(),
            Some(hex(seg_a).as_str())
        );
        assert_eq!(
            after.fittings[0].ports[VALVE_PORT_NAMES[1]].as_deref(),
            Some(hex(seg_b).as_str())
        );
        assert_eq!(after.fittings[0].handle, hex(valve), "阀门 handle 未变");
        assert_eq!(after.fittings[0].tag.as_deref(), Some("V-1001"), "位号未变");
    }

    /// `VAL-PIPE-022` 第一步：反向平移 -100 后仍满足 019 / 020 的全部判据。
    #[test]
    fn a_reverse_move_keeps_the_topology_intact() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        assert_eq!(move_valve(&mut app, valve, 40.0)["ok"], true);

        let back = move_valve(&mut app, valve, -100.0);
        assert_eq!(back["ok"], true, "{back}");
        assert!(close(insert_point(&app, valve)[0], 40.0), "阀门回到 40");

        let table = topology(&app);
        let a = &table.pipe_segments[0];
        let b = &table.pipe_segments[1];
        assert!(close(a.start.point[0], 0.0), "段 A 起点不动");
        assert!(close(b.end.point[0], 200.0), "段 B 终点不动");
        assert!(close(a.end.point[0], 26.0), "段 A 内侧端点跟到端口：{:?}", a.end.point);
        assert!(close(b.start.point[0], 54.0), "段 B 内侧端点跟到端口：{:?}", b.start.point);
        assert!(a.end.point[0] < b.start.point[0], "不重叠");
        let ports = placed_ports(&app, valve);
        let total = (a.end.point[0] - a.start.point[0])
            + (b.end.point[0] - b.start.point[0])
            + (ports[1].0 - ports[0].0);
        assert!((total - 200.0).abs() <= 1e-6, "长度守恒偏差：{total}");
    }

    /// `VAL-PIPE-022` 第二步：越界平移返回结构化失败，几何保持移动前状态。
    #[test]
    fn an_out_of_range_move_fails_without_partial_writes() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        assert_eq!(move_valve(&mut app, valve, 40.0)["ok"], true);
        let before = topology(&app);
        let before_point = insert_point(&app, valve);

        let refused = move_valve(&mut app, valve, 400.0);
        assert_eq!(refused["ok"], false, "越界必须被拒：{refused}");
        assert_eq!(refused["code"], REFILL_OUT_OF_RANGE, "{refused}");

        let after = topology(&app);
        assert_eq!(after, before, "失败时拓扑表一个字节都不该变");
        assert_eq!(insert_point(&app, valve), before_point, "阀门也不该动");
    }

    /// 只绑一个端口的管件（端点插入）移动时不补位、也不被拒。
    #[test]
    fn a_fitting_with_one_bound_port_is_not_refilled() {
        let mut app = app();
        draw(&mut app, "PIDPIPE", &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
        draw(&mut app, "PIDPIPE run", &[[0.0, 0.0, 0.0], [200.0, 0.0, 0.0]]);
        let done = draw(&mut app, "PIDFITTING gate_valve", &[[0.0, 0.0, 0.0]]);
        assert_eq!(done["ok"], true, "{done}");
        let table = topology(&app);
        assert_eq!(table.pipe_segments.len(), 1, "端点插管件不拆分");
        let valve = parse_handle(&table.fittings[0].handle).unwrap();
        let before = table.pipe_segments[0].clone();

        let moved = move_valve(&mut app, valve, 400.0);
        assert_eq!(moved["ok"], true, "单端口管件移动不触发补位校验：{moved}");
        let after = topology(&app);
        assert_eq!(after.pipe_segments[0], before, "唯一的管段不该被改写");
    }

    /// 平移普通实体不碰拓扑表。
    #[test]
    fn moving_a_non_fitting_entity_leaves_the_topology_alone() {
        let mut app = app();
        line_with_valve_at_midpoint(&mut app);
        let before = topology(&app);
        // 一条与 P&ID 无关的线。
        request(&mut app, json!({"op":"run","cmd":"LINE 0,300 100,300"}));
        let queried = request(&mut app, json!({"op":"query","type":"Line"}));
        let plain = queried["entities"]
            .as_array()
            .and_then(|list| list.last())
            .and_then(|entity| entity["handle"].as_str())
            .expect("新线有 handle")
            .to_string();
        let moved = request(
            &mut app,
            json!({
                "op":"entities_transform",
                "action":"move",
                "handles":[plain],
                "vector":[40,0,0],
            }),
        );
        assert_eq!(moved["ok"], true, "{moved}");
        assert_eq!(topology(&app), before, "普通实体移动不该改拓扑表");
    }

    // ── scene 层直调（不经控制面）：三条真实路径共享的同一份实现 ──────────

    /// 直接调 `Scene::transform_entities`（`MOVE` 命令与 grip 提交走的就是它），
    /// 断言两侧管段内侧端点吸附到阀门端口、管段数不变。
    #[test]
    fn scene_transform_entities_refills_the_segments_of_a_moved_valve() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        let i = app.active_tab;
        app.tabs[i].scene.transform_entities(
            &[valve],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(40.0, 0.0, 0.0)),
        );

        assert!(close(insert_point(&app, valve)[0], 140.0), "阀门应移到 140");
        let table = topology(&app);
        assert_eq!(table.pipe_runs[0].segments.len(), 2, "管段数不变");
        assert_eq!(table.pipe_segments.len(), 2, "不重建管段");
        let a = &table.pipe_segments[0];
        let b = &table.pipe_segments[1];
        let ports = placed_ports(&app, valve);
        assert!(close(a.end.point[0], ports[0].0), "段 A 内侧端点吸附：{:?}", a.end.point);
        assert!(close(b.start.point[0], ports[1].0), "段 B 内侧端点吸附：{:?}", b.start.point);
        assert!(close(a.start.point[0], 0.0) && close(b.end.point[0], 200.0), "外侧端点不动");
    }

    /// scene 层越界平移整体拒绝：几何与拓扑表都保持移动前状态。
    #[test]
    fn scene_transform_entities_refuses_an_out_of_range_move_whole() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        let i = app.active_tab;
        app.tabs[i].scene.transform_entities(
            &[valve],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(40.0, 0.0, 0.0)),
        );
        let before_table = topology(&app);
        let before_point = insert_point(&app, valve);
        let before_lines = line_points(&app, &before_table);

        let refused = app.tabs[i].scene.transform_entities_checked(
            &[valve],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(400.0, 0.0, 0.0)),
        );
        assert!(refused.is_err(), "越界必须被拒");
        assert_eq!(topology(&app), before_table, "拓扑表不变");
        assert_eq!(insert_point(&app, valve), before_point, "阀门不动");
        assert_eq!(line_points(&app, &before_table), before_lines, "管段几何不变");
    }

    /// 锁定图层上的阀门经 scene 层移动被整体拒绝，不产生半改状态。
    #[test]
    fn scene_transform_entities_refuses_a_valve_on_a_locked_layer() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        let i = app.active_tab;
        let layer = app.tabs[i].scene.document.get_entity(valve).unwrap().common().layer.clone();
        if let Some(record) = app.tabs[i].scene.document.layers.get_mut(&layer) {
            record.flags.locked = true;
        }
        let before_table = topology(&app);
        let before_point = insert_point(&app, valve);

        app.tabs[i].scene.transform_entities(
            &[valve],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(40.0, 0.0, 0.0)),
        );

        assert_eq!(insert_point(&app, valve), before_point, "锁定图层的阀门不动");
        assert_eq!(topology(&app), before_table, "拓扑表不变，无半改");
    }

    /// 每个管段实体的两端点坐标，按拓扑表里的 handle 顺序。
    fn line_points(app: &OpenCADStudio, table: &TopologyTable) -> Vec<([f64; 2], [f64; 2])> {
        table
            .pipe_segments
            .iter()
            .filter_map(|segment| {
                let handle = parse_handle(&segment.handle)?;
                match app.tabs[app.active_tab].scene.document.get_entity(handle) {
                    Some(codec::EntityType::Line(line)) => Some((
                        [line.start.x, line.start.y],
                        [line.end.x, line.end.y],
                    )),
                    _ => None,
                }
            })
            .collect()
    }

    /// 把视图框到 0..200 的管路范围，让 grip 拖动能把点投影到屏幕。
    fn frame_the_pipe(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 800.0);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-50.0, -50.0, 0.0),
            glam::DVec3::new(250.0, 50.0, 0.0),
            2.0,
        );
    }

    /// `grip_drag` op（`POST /api/v1/grip_drag` 的同一条路径）拖动阀门后补位
    /// 生效，结果与 REST `entities_transform` 通道一致。
    ///
    /// `POST /api/v1/input` 无法驱动拖动 —— 这里走的是真实的产品入口。
    #[test]
    fn the_grip_drag_op_refills_exactly_like_the_control_transform() {
        let mut dragged_app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut dragged_app);
        frame_the_pipe(&mut dragged_app);

        let dragged = request(
            &mut dragged_app,
            json!({
                "op":"grip_drag",
                "handle": hex(valve),
                "grip_id": 0,
                "point": [140.0, 0.0, 0.0],
            }),
        );
        assert_eq!(dragged["ok"], true, "{dragged}");
        assert!(
            near(insert_point(&dragged_app, valve)[0], 140.0),
            "阀门拖到 140，实际 {:?}：{dragged}",
            insert_point(&dragged_app, valve)
        );

        let dragged_table = topology(&dragged_app);
        let a = &dragged_table.pipe_segments[0];
        let b = &dragged_table.pipe_segments[1];
        // grip 落点经屏幕像素投影，误差在 1e-3 内。
        assert!(near(a.end.point[0], 126.0), "段 A 内侧端点：{:?}", a.end.point);
        assert!(near(b.start.point[0], 154.0), "段 B 内侧端点：{:?}", b.start.point);
        assert_eq!(dragged_table.pipe_runs[0].segments.len(), 2, "管段数不变");
        let geometry = line_points(&dragged_app, &dragged_table);
        assert_eq!(geometry.len(), 2, "两段都在");
        assert!(near(geometry[0].0[0], 0.0) && near(geometry[0].1[0], 126.0), "{geometry:?}");
        assert!(near(geometry[1].0[0], 154.0) && near(geometry[1].1[0], 200.0), "{geometry:?}");
        // 管段实体几何与拓扑表一致。
        for (index, (start, end)) in geometry.iter().enumerate() {
            let record = &dragged_table.pipe_segments[index];
            assert!(near(start[0], record.start.point[0]), "{geometry:?}");
            assert!(near(end[0], record.end.point[0]), "{geometry:?}");
        }

        // 与 REST 通道同一场景比对：另一条独立管路走 entities_transform。
        let mut control_app = app();
        let (other_valve, _, _) = line_with_valve_at_midpoint(&mut control_app);
        assert_eq!(move_valve(&mut control_app, other_valve, 40.0)["ok"], true);
        let control_table = topology(&control_app);
        assert_eq!(
            control_table.pipe_runs[0].segments.len(),
            dragged_table.pipe_runs[0].segments.len(),
            "两条通道的管段数一致"
        );
        for (dragged_segment, control_segment) in dragged_table
            .pipe_segments
            .iter()
            .zip(control_table.pipe_segments.iter())
        {
            assert!(near(dragged_segment.start.point[0], control_segment.start.point[0]));
            assert!(near(dragged_segment.end.point[0], control_segment.end.point[0]));
        }
    }

    /// 越界拖动被整体拒绝：grip 回原位，几何与拓扑表都不变。
    #[test]
    fn an_out_of_range_grip_drag_is_cancelled_without_partial_writes() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        frame_the_pipe(&mut app);
        let before_table = topology(&app);
        let before_point = insert_point(&app, valve);
        let before_lines = line_points(&app, &before_table);

        let dragged = request(
            &mut app,
            json!({
                "op":"grip_drag",
                "handle": hex(valve),
                "grip_id": 0,
                "point": [900.0, 0.0, 0.0],
            }),
        );
        assert_eq!(dragged["ok"], false, "越界拖动必须报错：{dragged}");
        let message = dragged["error"].as_str().unwrap_or_default();
        assert!(message.contains("补位"), "错误要说明补位被拒：{dragged}");
        assert_eq!(insert_point(&app, valve), before_point, "阀门回到原位");
        assert_eq!(topology(&app), before_table, "拓扑表不变");
        assert_eq!(line_points(&app, &before_table), before_lines, "管段几何不变");
    }

    /// `MOVE` 命令路径：选中阀门后 `MOVE` + 两个点，补位与 REST 一致。
    #[test]
    fn the_move_command_refills_like_the_control_transform() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        request(
            &mut app,
            json!({"op":"select","handles":[hex(valve)],"replace":true}),
        );
        let started = request(&mut app, json!({"op":"start","cmd":"MOVE"}));
        assert_eq!(started["status"], "waiting_input", "{started}");
        for point in [[100.0, 0.0, 0.0], [140.0, 0.0, 0.0]] {
            let fed = request(
                &mut app,
                json!({"op":"input","kind":"point","point":point,"space":"wcs"}),
            );
            assert_eq!(fed["ok"], true, "{fed}");
        }

        assert!(close(insert_point(&app, valve)[0], 140.0), "MOVE 把阀门移到 140");
        let table = topology(&app);
        let a = &table.pipe_segments[0];
        let b = &table.pipe_segments[1];
        assert!(close(a.end.point[0], 126.0), "段 A 内侧端点：{:?}", a.end.point);
        assert!(close(b.start.point[0], 154.0), "段 B 内侧端点：{:?}", b.start.point);
        assert!(close(a.start.point[0], 0.0) && close(b.end.point[0], 200.0), "外侧端点不动");
        assert_eq!(table.pipe_runs[0].segments.len(), 2, "管段数不变");
    }

    /// `MOVE` 命令越界时整条命令被放弃：几何与拓扑表都不变，命令报错。
    #[test]
    fn the_move_command_refuses_an_out_of_range_move_whole() {
        let mut app = app();
        let (valve, _, _) = line_with_valve_at_midpoint(&mut app);
        let before_table = topology(&app);
        let before_point = insert_point(&app, valve);
        request(
            &mut app,
            json!({"op":"select","handles":[hex(valve)],"replace":true}),
        );
        let started = request(&mut app, json!({"op":"start","cmd":"MOVE"}));
        assert_eq!(started["status"], "waiting_input", "{started}");
        let _ = request(
            &mut app,
            json!({"op":"input","kind":"point","point":[100.0, 0.0, 0.0],"space":"wcs"}),
        );
        let done = request(
            &mut app,
            json!({"op":"input","kind":"point","point":[600.0, 0.0, 0.0],"space":"wcs"}),
        );
        assert_eq!(done["ok"], false, "越界 MOVE 必须报错：{done}");
        let message = done["error"].as_str().unwrap_or_default();
        assert!(message.contains("补位"), "错误要说明补位被拒：{done}");

        assert_eq!(insert_point(&app, valve), before_point, "阀门不动");
        assert_eq!(topology(&app), before_table, "拓扑表不变，无半改");
        assert_eq!(line_points(&app, &before_table).len(), 2);
    }
}
