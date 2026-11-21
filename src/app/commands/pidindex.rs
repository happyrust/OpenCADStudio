//! PIDINDEX：项目索引（`pid::project_index`，`SPEC-pid-project-index.md` §4、§5.2）。
//!
//! ```text
//! PIDINDEX              几张图（几张读不出来）、跨图重号几处、连接符配成几对、几个没配上
//! PIDINDEX DUPLICATES   逐条列出跨图重号：类型、位号、各在哪张图的哪里
//! PIDINDEX LINKS        逐条列出连接符：配成对的，和没配上的及其 status
//! ```
//!
//! 项目目录是当前图所在的目录（§3.7），当前图没保存过就没有项目目录
//! （`INDEX_NO_PROJECT_DIR`）。目录里的图正在程序里打开着的，摘要它内存里的那份，含没
//! 保存的修改，每次现做；没打开的读磁盘文件，摘要按「路径 + 大小 + 修改时间」记在内存
//! 里（[`SummaryCache`]），文件没变就不再读。不往项目目录里写任何东西（§4）。
//!
//! 扫描眼下是同步的，扫完才回话；挪到后台任务是 `IX-6` 的事。列图、建索引出错时连同
//! 错误码一起说（`INDEX_DRAWING_ID_DUPLICATED`、项目文件的 `MASTERDATA_...`、
//! `INDEX_IO_ERROR`），命令行的错误码也设上。三个命令都不改图，只读会话里照常能用。

use super::*;
use crate::pid::project_index::{
    build_index, list_drawings, summarize, DrawingFile, DrawingSummary, FailedDrawing, Fingerprint,
    IndexError, LinkEnd, LinkFamily, LinkStatus, ProjectIndex, SummaryMeta, SummarySource,
    DEFAULT_TOLERANCE,
};
use crate::pid::symbols::SymbolLibrary;
use document::Document;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 当前图没保存过，没有项目目录（§5.3）。
pub(in crate::app) const INDEX_NO_PROJECT_DIR: &str = "INDEX_NO_PROJECT_DIR";

/// 磁盘上的图的摘要，按路径记（§4）。文件的大小和修改时间都没变才拿来用；程序退出即
/// 清空。
#[derive(Default)]
pub(in crate::app) struct SummaryCache {
    entries: HashMap<PathBuf, Cached>,
}

/// 缓存里的一张图：读它时文件的大小和修改时间，和做出来的摘要。
struct Cached {
    size: u64,
    /// 文件系统给的修改时间，不截到秒：同一秒里存了两次也认得出来。
    modified: SystemTime,
    summary: DrawingSummary,
}

impl SummaryCache {
    /// 磁盘上一张图的摘要：文件没变就用记着的，否则用 `load` 读来现做并记下。读不出来
    /// 的不记，下次再读——文件可能只是一时被别的程序占着。
    fn summary(
        &mut self,
        drawing: &DrawingFile,
        library: &SymbolLibrary,
        load: &mut dyn FnMut(&Path) -> Result<Document, String>,
    ) -> Result<DrawingSummary, String> {
        let metadata = std::fs::metadata(&drawing.path).map_err(|error| error.to_string())?;
        let size = metadata.len();
        let modified = metadata.modified().ok();
        if let Some(cached) = self.entries.get(&drawing.path) {
            if cached.size == size && Some(cached.modified) == modified {
                // 图号和 PBS 节点取项目文件现在写的：项目文件改了，图本身可以没变。
                let mut summary = cached.summary.clone();
                summary.drawing_id.clone_from(&drawing.drawing_id);
                summary.pbs.clone_from(&drawing.pbs);
                return Ok(summary);
            }
        }
        let document = load(&drawing.path).inspect_err(|_| {
            self.entries.remove(&drawing.path);
        })?;
        let fingerprint = modified.map(|modified| Fingerprint {
            size,
            modified_at: unix_seconds(modified),
        });
        let summary = summarize(
            &document,
            library,
            meta(drawing, SummarySource::File, fingerprint),
            DEFAULT_TOLERANCE,
        );
        if let Some(modified) = modified {
            let cached = Cached {
                size,
                modified,
                summary: summary.clone(),
            };
            self.entries.insert(drawing.path.clone(), cached);
        } else {
            self.entries.remove(&drawing.path);
        }
        Ok(summary)
    }

    /// 去掉 `project_dir` 里这次没列到的文件：删了、改了名的不再占着内存。
    fn forget_unlisted(&mut self, project_dir: &Path, drawings: &[DrawingFile]) {
        let listed: HashSet<&Path> = drawings
            .iter()
            .map(|drawing| drawing.path.as_path())
            .collect();
        self.entries.retain(|path, _| {
            path.parent() != Some(project_dir) || listed.contains(path.as_path())
        });
    }
}

/// 扫一遍项目目录（§3.7、§4），得出项目索引和读得出来的各图的摘要。
///
/// `open` 是正在程序里打开着的图和它们的文件：目录里的图在其中的，摘要这一份（含没保存
/// 的修改），不读文件，也不进缓存。其余的经 `cache` 读磁盘，`load` 是读一个文件的办法
/// （命令用 `crate::io::load_file`，测试换成数次数的）。读不出来的图带着原因进索引。目录
/// 里已经没有的文件从 `cache` 里去掉。
pub(in crate::app) fn scan(
    project_dir: &Path,
    open: &[(&Path, &Document)],
    library: &SymbolLibrary,
    cache: &mut SummaryCache,
    load: &mut dyn FnMut(&Path) -> Result<Document, String>,
) -> Result<(ProjectIndex, Vec<DrawingSummary>), IndexError> {
    let drawings = list_drawings(project_dir)?;
    let open: Vec<(PathBuf, &Document)> = open
        .iter()
        .map(|&(path, document)| (same_file(path), document))
        .collect();
    let mut summaries = Vec::with_capacity(drawings.len());
    let mut failed = Vec::new();
    for drawing in &drawings {
        let shown = if open.is_empty() {
            None
        } else {
            let path = same_file(&drawing.path);
            open.iter()
                .find(|(open_path, _)| *open_path == path)
                .map(|&(_, document)| document)
        };
        let summary = if let Some(document) = shown {
            Ok(summarize(
                document,
                library,
                meta(drawing, SummarySource::Open, None),
                DEFAULT_TOLERANCE,
            ))
        } else {
            cache.summary(drawing, library, load)
        };
        match summary {
            Ok(summary) => summaries.push(summary),
            Err(error) => failed.push(FailedDrawing {
                drawing_id: drawing.drawing_id.clone(),
                file: drawing.file.clone(),
                pbs: drawing.pbs.clone(),
                error,
            }),
        }
    }
    cache.forget_unlisted(project_dir, &drawings);
    let index = build_index(project_dir, &summaries, &failed)?;
    Ok((index, summaries))
}

fn meta(
    drawing: &DrawingFile,
    source: SummarySource,
    fingerprint: Option<Fingerprint>,
) -> SummaryMeta {
    SummaryMeta {
        drawing_id: drawing.drawing_id.clone(),
        file: Some(drawing.file.clone()),
        pbs: drawing.pbs.clone(),
        source,
        fingerprint,
    }
}

/// Unix 秒；1970 年以前的算 0。
fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
    })
}

/// 认同一个文件用的路径：解析得了就用解析后的（大小写、`..`、斜杠方向都归一），否则
/// 原样。
fn same_file(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidindex(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let mut words = cmd.split_whitespace();
        if !words
            .next()
            .is_some_and(|verb| verb.eq_ignore_ascii_case("PIDINDEX"))
        {
            return None;
        }
        let option = words.next().unwrap_or_default().to_ascii_uppercase();
        let rest: Vec<&str> = words.collect();
        if !matches!(option.as_str(), "" | "DUPLICATES" | "LINKS") {
            self.command_line.push_error(&format!(
                "PIDINDEX: 不认识 {option}；写 DUPLICATES 或 LINKS，或者只写 PIDINDEX。"
            ));
            return Some(Task::none());
        }
        if !rest.is_empty() {
            self.command_line.push_error(&format!(
                "PIDINDEX {option}: 后面不用再写东西，多了 {}。",
                rest.join(" ")
            ));
            return Some(Task::none());
        }
        let Some(index) = self.pidindex_scan(i) else {
            return Some(Task::none());
        };
        match option.as_str() {
            "DUPLICATES" => self.pidindex_duplicates(&index),
            "LINKS" => self.pidindex_links(&index),
            _ => self.pidindex_overview(&index),
        }
        Some(Task::none())
    }

    /// 扫当前图所在的项目目录。没有项目目录、列图或建索引出错时在命令行连同错误码说，
    /// 返回 `None`。打开着的图以标签页图纸的 `source_path` 认，与定项目目录用的是同一个。
    fn pidindex_scan(&mut self, i: usize) -> Option<ProjectIndex> {
        let Some(project_dir) =
            crate::io::drawing_dir(&self.tabs[i].scene.document).map(Path::to_path_buf)
        else {
            self.command_line.push_error_coded(
                INDEX_NO_PROJECT_DIR,
                &format!(
                    "PIDINDEX: {INDEX_NO_PROJECT_DIR}: 当前图没保存过，不属于哪个项目；存盘后再扫。"
                ),
            );
            return None;
        };
        let open: Vec<(&Path, &Document)> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                let document = &tab.scene.document;
                Some((Path::new(document.source_path.as_deref()?), document))
            })
            .collect();
        let scanned = scan(
            &project_dir,
            &open,
            &self.pid_symbols,
            &mut self.pid_index_cache,
            &mut crate::io::load_file,
        );
        match scanned {
            Ok((index, _)) => Some(index),
            Err(error) => {
                self.command_line
                    .push_error_coded(error.code(), &format!("PIDINDEX: {error}"));
                None
            }
        }
    }

    /// 不带参数：几张图（几张读不出来）、跨图重号几处、连接符配成几对、几个没配上。
    /// 读不出来的图逐张说原因，用警告说：索引本身建成了（§3.7）。
    fn pidindex_overview(&mut self, index: &ProjectIndex) {
        let unreadable: Vec<_> = index
            .drawings
            .iter()
            .filter(|drawing| drawing.error.is_some())
            .collect();
        let note = if unreadable.is_empty() {
            String::new()
        } else {
            format!("（{} 张读不出来）", unreadable.len())
        };
        self.command_line.push_output(&format!(
            "PIDINDEX: 项目目录 {}：{} 张图{note}，跨图重号 {} 处，连接符配成 {} 对、{} 个没配上。",
            index.project_dir,
            index.drawings.len(),
            index.duplicates.len(),
            index.links.len(),
            index.unresolved.len()
        ));
        for drawing in unreadable {
            self.command_line.push_warning(&format!(
                "{}（{}）读不出来：{}",
                drawing.drawing_id,
                drawing.file.as_deref().unwrap_or_default(),
                drawing.error.as_deref().unwrap_or_default()
            ));
        }
        if !(index.duplicates.is_empty() && index.links.is_empty() && index.unresolved.is_empty()) {
            self.command_line
                .push_info("PIDINDEX DUPLICATES 逐条列跨图重号，PIDINDEX LINKS 逐条列连接符。");
        }
    }

    /// DUPLICATES：逐条列出跨图重号，类型、位号、各在哪张图的哪里。
    fn pidindex_duplicates(&mut self, index: &ProjectIndex) {
        if index.duplicates.is_empty() {
            self.command_line
                .push_output("PIDINDEX DUPLICATES: 没有跨图重号。");
        } else {
            self.command_line.push_output(&format!(
                "PIDINDEX DUPLICATES: 跨图重号 {} 处。",
                index.duplicates.len()
            ));
        }
        for duplicate in &index.duplicates {
            let places: Vec<String> = duplicate
                .places
                .iter()
                .map(|place| format!("{} {}", place.drawing_id, point(place.at)))
                .collect();
            self.command_line.push_output(&format!(
                "  {} {}：{}",
                duplicate.type_code,
                duplicate.tag,
                places.join("、")
            ));
        }
        self.pidindex_incomplete(index);
    }

    /// LINKS：逐条列出配成对的连接符（流出侧 → 流入侧），和没配上的及其 `status`。
    fn pidindex_links(&mut self, index: &ProjectIndex) {
        self.command_line.push_output(&format!(
            "PIDINDEX LINKS: 配成 {} 对，{} 个没配上。",
            index.links.len(),
            index.unresolved.len()
        ));
        for link in &index.links {
            let family = match link.family {
                LinkFamily::Pipe => "管道",
                LinkFamily::Signal => "信号",
            };
            self.command_line.push_output(&format!(
                "  {family}：{} → {}",
                link_end(&link.from),
                link_end(&link.to)
            ));
        }
        for connector in &index.unresolved {
            let (status, meaning) = match connector.status {
                LinkStatus::OneSided => ("one_sided", "记着配过对，对面没找到"),
                LinkStatus::TargetMissing => ("target_missing", "指向的图不在项目里"),
                LinkStatus::Unpaired => ("unpaired", "没配过对"),
                LinkStatus::Ambiguous => ("ambiguous", "对面不止一个，分不出是哪个"),
            };
            let target = connector
                .target
                .as_deref()
                .map_or_else(String::new, |target| format!("，指向 {target}"));
            self.command_line.push_output(&format!(
                "  没配上：{} {} {} {}：{status}（{meaning}{target}）",
                connector.drawing_id,
                connector.kind,
                connector.line,
                point(connector.at)
            ));
        }
        self.pidindex_incomplete(index);
    }

    /// 有图读不出来时提一句：逐条列的不含那几张。
    fn pidindex_incomplete(&mut self, index: &ProjectIndex) {
        if !index.complete {
            let unreadable = index
                .drawings
                .iter()
                .filter(|drawing| drawing.error.is_some())
                .count();
            self.command_line.push_warning(&format!(
                "有 {unreadable} 张读不出来的图没算在里面，PIDINDEX 说是哪几张。"
            ));
        }
    }
}

fn link_end(end: &LinkEnd) -> String {
    format!("{} {} {}", end.drawing_id, end.line, point(end.at))
}

/// `(x, y)`，最多三位小数，去掉末尾的 0。
fn point([x, y]: [f64; 2]) -> String {
    format!("({}, {})", number(x), number(y))
}

fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pid::master_data::PROJECT_FILE;
    use crate::pid::project_index::synthetic_drawing;
    use crate::pid::topology::TopologyTable;
    use serde_json::{json, Value};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// 测试用的临时目录，用完删掉。
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            // 测试并行跑：同一纳秒里建的两个靠计数分开。
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "ocs-pidindex-{}-{}-{nanos}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("临时目录");
            Self(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 一行命令说了什么，报了什么错：`error` 是那行错，`code` 是它的错误码，没报错都是
    /// `None`。
    struct Ran {
        said: String,
        error: Option<String>,
        code: Option<String>,
    }

    /// 像在命令行上敲进去那样跑一行。
    fn run(app: &mut OpenCADStudio, line: &str) -> Ran {
        app.command_line.history.clear();
        app.command_line.last_error = None;
        app.command_line.last_error_code = None;
        let _ = app.run_command_line(line);
        app.finish_all_pending_history();
        Ran {
            said: app
                .command_line
                .history
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            error: app.command_line.last_error.clone(),
            code: app.command_line.last_error_code.clone(),
        }
    }

    fn undo_steps(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    /// 协议 1 的一条控制请求，发给当前的图。
    fn request(app: &mut OpenCADStudio, mut req: Value) -> Value {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        req["protocol"] = json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["client_id"] = json!("test");
        req["request_id"] = json!(format!(
            "pidindex-{}",
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        app.automation_op(&req.to_string())
    }

    /// 起 `cmd`，依次喂点，最后回车。
    fn draw(app: &mut OpenCADStudio, cmd: &str, points: &[[f64; 2]]) {
        let started = request(app, json!({"op": "run", "cmd": cmd}));
        assert_eq!(started["ok"], true, "{cmd}：{started}");
        for [x, y] in points {
            let point =
                json!({"op": "input", "kind": "point", "point": [x, y, 0.0], "space": "wcs"});
            let fed = request(app, point);
            assert_eq!(fed["ok"], true, "{cmd} 喂点：{fed}");
        }
        request(app, json!({"op": "input", "kind": "enter"}));
    }

    /// 当前图画成项目里的一张小图：`pumps` 台泵（位号自动编，两张图上编出来的一样），
    /// 一根管线 L-1001，线头上一个 `connector`，记成与 `peer` 那张图由服务器配过对
    /// （两个线号都记 L-1001）。
    fn sheet(app: &mut OpenCADStudio, pumps: usize, connector: &str, at: [f64; 2], peer: &str) {
        for spot in &[[0.0, 0.0], [0.0, -60.0]][..pumps] {
            draw(app, "PIDINSERT pump", &[*spot]);
        }
        draw(app, "PIDPIPE", &[[0.0, 100.0], [100.0, 100.0]]);
        draw(app, &format!("PIDCONNECTOR {connector}"), &[at]);
        let i = app.active_tab;
        let document = &mut app.tabs[i].scene.document;
        let mut table = TopologyTable::load(document);
        assert_eq!(table.connectors.len(), 1, "{:#?}", table.connectors);
        for record in &mut table.connectors {
            record.peer_drawing = Some(peer.to_string());
            record.upstream = Some("L-1001".to_string());
            record.downstream = Some("L-1001".to_string());
        }
        table.store(document);
    }

    /// 把当前图存成 `path`。
    fn save(app: &OpenCADStudio, path: &Path) {
        let bytes = crate::io::save_to_bytes(
            app.active_document_for_test(),
            "dwg",
            document::DxfVersion::AC1032,
        )
        .expect("存 DWG");
        fs::write(path, bytes).expect("写文件");
    }

    /// CP-B 的样子：临时目录里两张图。PID-1101 上一台泵，PID-1102 上两台，头一台的位号
    /// 两张图上相同；PID-1101 的流出连接符与 PID-1102 的流入连接符配成一对。当前图是
    /// PID-1101，存过盘、打开着；PID-1102 只在磁盘上。
    fn project(scratch: &Scratch) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        sheet(&mut app, 2, "pipe_in", [0.0, 100.0], "PID-1101");
        save(&app, &scratch.path("PID-1102.dwg"));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        sheet(&mut app, 1, "pipe_out", [100.0, 100.0], "PID-1102");
        let current = scratch.path("PID-1101.dwg");
        save(&app, &current);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = Some(current.to_string_lossy().into_owned());
        app
    }

    /// 两张图（§7 命令与操作）：`PIDINDEX` 说出图数、重号数、配对数，`DUPLICATES` 和
    /// `LINKS` 逐条列出。
    #[test]
    fn pidindex_counts_the_drawings_duplicates_and_links() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);

        let ran = run(&mut app, "PIDINDEX");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(
            ran.said
                .contains("2 张图，跨图重号 1 处，连接符配成 1 对、0 个没配上"),
            "{}",
            ran.said
        );

        let ran = run(&mut app, "PIDINDEX DUPLICATES");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(ran.said.contains("跨图重号 1 处"), "{}", ran.said);
        assert!(
            ran.said.contains("：PID-1101 (0, 0)、PID-1102 (0, 0)"),
            "{}",
            ran.said
        );

        let ran = run(&mut app, "PIDINDEX LINKS");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(ran.said.contains("配成 1 对，0 个没配上"), "{}", ran.said);
        assert!(
            ran.said
                .contains("管道：PID-1101 L-1001 (100, 100) → PID-1102 L-1001 (0, 100)"),
            "{}",
            ran.said
        );
    }

    /// 没有索引时说为什么，连同错误码：当前图没保存过、项目文件让两个文件同号、项目目录
    /// 读不了。
    #[test]
    fn pidindex_says_why_there_is_no_index() {
        let scratch = Scratch::new();
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = None;
        let ran = run(&mut app, "PIDINDEX");
        assert_eq!(
            ran.code.as_deref(),
            Some(INDEX_NO_PROJECT_DIR),
            "{}",
            ran.said
        );
        assert!(
            ran.error.is_some_and(|error| error.starts_with("PIDINDEX")),
            "{}",
            ran.said
        );

        for name in ["PID-1101.dwg", "PID-1102.dwg"] {
            fs::write(scratch.path(name), "").expect("写文件");
        }
        let project = json!({
            "version": 1,
            "drawings": [{ "file": "PID-1101.dwg", "drawing_id": "PID-1102" }]
        });
        fs::write(scratch.path(PROJECT_FILE), project.to_string()).expect("项目文件");
        app.tabs[i].scene.document.source_path =
            Some(scratch.path("PID-1101.dwg").to_string_lossy().into_owned());
        let ran = run(&mut app, "PIDINDEX LINKS");
        assert_eq!(
            ran.code.as_deref(),
            Some("INDEX_DRAWING_ID_DUPLICATED"),
            "{}",
            ran.said
        );
        assert!(
            ran.error
                .is_some_and(|error| error.starts_with("PIDINDEX") && error.contains("PID-1102")),
            "{}",
            ran.said
        );

        let nowhere = scratch.path("nowhere").join("PID-1101.dwg");
        app.tabs[i].scene.document.source_path = Some(nowhere.to_string_lossy().into_owned());
        let ran = run(&mut app, "PIDINDEX DUPLICATES");
        assert_eq!(ran.code.as_deref(), Some("INDEX_IO_ERROR"), "{}", ran.said);
    }

    /// 扫一遍 `dir`，记下这次读了哪些文件。
    fn scan_counting(
        dir: &Path,
        open: &[(&Path, &Document)],
        library: &SymbolLibrary,
        cache: &mut SummaryCache,
    ) -> (ProjectIndex, Vec<String>) {
        let mut read = Vec::new();
        let mut load = |path: &Path| {
            read.push(
                path.file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            );
            crate::io::load_file(path)
        };
        let (index, _) = scan(dir, open, library, cache, &mut load).expect("扫描");
        (index, read)
    }

    fn objects(index: &ProjectIndex, drawing_id: &str) -> Option<usize> {
        index
            .drawings
            .iter()
            .find(|drawing| drawing.drawing_id == drawing_id)
            .and_then(|drawing| drawing.objects)
    }

    /// 缓存（§4）：文件没变时第二次扫描一个也不重读；改了一个只重读那一个；读不出来的
    /// 不记，每次再读；打开着的图摘要内存里那份，不读它的文件；目录里没了的从缓存里去掉。
    #[test]
    fn a_second_scan_reads_only_the_files_that_changed() {
        let scratch = Scratch::new();
        let library = SymbolLibrary::builtin();
        let write = |name: &str, objects: usize| {
            let bytes = crate::io::save_to_bytes(
                &synthetic_drawing(objects),
                "dwg",
                document::DxfVersion::AC1032,
            )
            .expect("存 DWG");
            fs::write(scratch.path(name), bytes).expect("写文件");
        };
        write("A.dwg", 20);
        write("B.dwg", 20);
        fs::write(scratch.path("C.dwg"), "不是图纸").expect("写文件");
        let mut cache = SummaryCache::default();

        let (first, read) = scan_counting(&scratch.0, &[], &library, &mut cache);
        assert_eq!(read, ["A.dwg", "B.dwg", "C.dwg"]);
        assert!(!first.complete, "{first:#?}");
        assert!(objects(&first, "A").is_some(), "{first:#?}");
        assert_eq!(objects(&first, "C"), None, "{first:#?}");

        let (again, read) = scan_counting(&scratch.0, &[], &library, &mut cache);
        assert_eq!(read, ["C.dwg"], "没变的不重读，读不出来的再读");
        assert_eq!(again.drawings, first.drawings);
        assert_eq!(again.duplicates, first.duplicates);

        write("A.dwg", 40);
        let later = SystemTime::now() + Duration::from_secs(10);
        fs::File::options()
            .write(true)
            .open(scratch.path("A.dwg"))
            .and_then(|file| file.set_modified(later))
            .expect("改修改时间");
        let (changed, read) = scan_counting(&scratch.0, &[], &library, &mut cache);
        assert_eq!(read, ["A.dwg", "C.dwg"]);
        assert_ne!(objects(&changed, "A"), objects(&first, "A"));

        let edited = synthetic_drawing(60);
        let b = scratch.path("B.dwg");
        let open = [(b.as_path(), &edited)];
        let (opened, read) = scan_counting(&scratch.0, &open, &library, &mut cache);
        assert_eq!(read, ["C.dwg"], "打开着的不读文件");
        let b = opened
            .drawings
            .iter()
            .find(|drawing| drawing.drawing_id == "B")
            .expect("B");
        assert_eq!(b.source, SummarySource::Open);
        assert_ne!(b.objects, objects(&first, "B"));

        fs::remove_file(scratch.path("A.dwg")).expect("删文件");
        let (_, read) = scan_counting(&scratch.0, &[], &library, &mut cache);
        assert_eq!(read, ["C.dwg"]);
        assert_eq!(cache.entries.len(), 1, "只剩 B");
    }

    /// 打开着的图里没保存的修改，索引里看得到：当前图上再插一台泵（位号与 PID-1102 上
    /// 第二台的相同），不存盘，跨图重号多一处；磁盘上的文件没动。
    #[test]
    fn pidindex_sees_what_the_open_drawing_has_not_saved() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let file = scratch.path("PID-1101.dwg");
        let before = fs::read(&file).expect("读文件");
        let ran = run(&mut app, "PIDINDEX");
        assert!(ran.said.contains("跨图重号 1 处"), "{}", ran.said);

        draw(&mut app, "PIDINSERT pump", &[[0.0, -60.0]]);
        let ran = run(&mut app, "PIDINDEX");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(ran.said.contains("跨图重号 2 处"), "{}", ran.said);
        assert_eq!(fs::read(&file).expect("读文件"), before, "没存盘");
    }

    /// 只读会话里三个命令都能用，不留撤销步。
    #[test]
    fn pidindex_runs_in_a_readonly_session() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        app.collab.readonly = true;
        let steps = undo_steps(&app);
        for (line, said) in [
            ("PIDINDEX", "PIDINDEX: 项目目录"),
            ("PIDINDEX DUPLICATES", "PIDINDEX DUPLICATES:"),
            ("pidindex links", "PIDINDEX LINKS:"),
        ] {
            assert!(!app.readonly_refuses_command(line), "{line}");
            let ran = run(&mut app, line);
            assert_eq!(ran.error, None, "{line}: {}", ran.said);
            assert!(ran.said.contains(said), "{line}: {}", ran.said);
        }
        assert_eq!(undo_steps(&app), steps);
    }

    /// 命令行上的 `PIDINDEX LINKS` 整行是一条命令，只跑一次：不先拿裸动词探一遍（那会先
    /// 说一遍总数）。
    #[test]
    fn pidindex_links_from_the_command_line_runs_once() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let ran = run(&mut app, "PIDINDEX LINKS");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(
            ran.said.matches("PIDINDEX LINKS:").count(),
            1,
            "{}",
            ran.said
        );
        assert!(
            !ran.said.contains("张图"),
            "裸的 PIDINDEX 先跑了：{}",
            ran.said
        );
    }

    #[test]
    fn pidindex_is_registered_and_says_how_it_is_written() {
        assert!(crate::command::all_registered_command_names().contains(&"PIDINDEX"));
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for line in [
            "PIDINDEX FROB",
            "PIDINDEX LINKS EVERYTHING",
            "pidindex duplicates now",
        ] {
            let ran = run(&mut app, line);
            assert_eq!(ran.code, None, "{line}: {}", ran.said);
            assert!(
                ran.error.is_some_and(|error| error.starts_with("PIDINDEX")),
                "{line}: {}",
                ran.said
            );
        }
    }
}
