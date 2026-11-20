//! PIDMASTERDATA：项目主数据（`pid::master_data`，`SPEC-pid-master-data.md` §5.2）。
//!
//! ```text
//! PIDMASTERDATA [STATUS]                        生效的是哪一份、修订号、各表多少条
//! PIDMASTERDATA INIT [PROJECT|DRAWING]          建一份默认主数据
//! PIDMASTERDATA EXPORT <文件>                   把生效的主数据写成一个新的 JSON 文件
//! PIDMASTERDATA IMPORT <文件> [PROJECT|DRAWING] 校验后整份替换目标位置的主数据
//! PIDMASTERDATA EMBED                           把项目文件里生效的主数据抄进图内记录
//! ```
//!
//! 不写目标时，图纸存过就写项目文件（图纸所在目录的 `pid-project.json`），没存过就写
//! 图内记录。文件名是这一行剩下的部分，可以带空格，也可以加双引号；不是完整路径时从
//! 图纸所在目录算起，没存过的图从当前目录算起。IMPORT 的文件名后面跟着 `PROJECT`
//! 或 `DRAWING` 时那是目标，所以文件名本身以这两个词之一结尾时要加引号。
//!
//! 出错时连同错误码一起说（§3.6 的 `MASTERDATA_...`），命令行的错误码也设上，REST 的
//! 调用方据此判断。写图内记录是一个撤销步；写项目文件、导出文件不进撤销。只读会话里
//! 只有 STATUS 和 EXPORT 能用（`readonly_refuses_command`）：项目文件不在撤销步里，
//! 只读会话拦编辑的那道锁管不到它，命令入口这一道就是全部。

use super::*;
use crate::pid::master_data::{self, MasterData, MasterDataError, Resolved, Source, PROJECT_FILE};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// 目标位置已经有主数据，或导出的目标文件已存在（§3.6）。
pub(in crate::app) const MASTERDATA_EXISTS: &str = "MASTERDATA_EXISTS";
/// 要导出或嵌入，但主数据未配置（§3.6）。
pub(in crate::app) const MASTERDATA_NOT_CONFIGURED: &str = "MASTERDATA_NOT_CONFIGURED";

/// 主数据写到哪里（§3.2）。
enum Target {
    /// 图纸所在目录里的项目文件；带着那个目录。
    Project(PathBuf),
    Drawing,
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidmasterdata(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = first_word(cmd);
        if !verb.eq_ignore_ascii_case("PIDMASTERDATA") {
            return None;
        }
        let (option, rest) = first_word(rest);
        match option.to_ascii_uppercase().as_str() {
            "" | "STATUS" => self.pidmasterdata_status(i),
            "INIT" => self.pidmasterdata_init(i, rest),
            "EXPORT" => self.pidmasterdata_export(i, rest),
            "IMPORT" => self.pidmasterdata_import(i, rest),
            "EMBED" => self.pidmasterdata_embed(i),
            _ => self.command_line.push_error(&format!(
                "PIDMASTERDATA: 不认识 {option}；写 STATUS、INIT、EXPORT、IMPORT 或 EMBED。"
            )),
        }
        Some(Task::none())
    }

    /// STATUS：生效的是哪一份、修订号、各表多少条。读不出来时说错在哪里，用警告说：
    /// 读状态这件事本身成了（§5.3）。项目文件生效而图内也有一份时，说图内那份没在用。
    fn pidmasterdata_status(&mut self, i: usize) {
        let document = &self.tabs[i].scene.document;
        let resolved = master_data::resolve(document, crate::io::drawing_dir(document));
        let in_drawing = master_data::load_from_drawing(document).is_some();
        match resolved {
            Resolved::Configured { data, source } => {
                self.command_line.push_output(&format!(
                    "PIDMASTERDATA: 生效的是{}，修订 {}。",
                    place(&source),
                    data.revision
                ));
                self.command_line
                    .push_output(&format!("  {}", contents(&data)));
                if in_drawing && source != Source::Drawing {
                    self.command_line
                        .push_output("  图内也有一份主数据，没在用：项目文件优先。");
                }
            }
            Resolved::Unreadable { source, error } => {
                self.command_line.push_warning(&format!(
                    "PIDMASTERDATA: {}读不出来，主数据按读不出来处理：{error}",
                    place(&source)
                ));
                if in_drawing && source != Source::Drawing {
                    self.command_line
                        .push_output("  图内也有一份主数据，不拿它顶替。");
                }
            }
            Resolved::NotConfigured => self.command_line.push_info(
                "PIDMASTERDATA: 未配置主数据。PIDMASTERDATA INIT 建一份默认的，\
                 或用 PIDMASTERDATA IMPORT <文件> 导入。",
            ),
        }
    }

    /// INIT：在目标位置建一份默认主数据，图例标准取符号库当前的。那里已有主数据是
    /// `MASTERDATA_EXISTS`；那里的读不出来时报读的那个错，也不动它——整份换掉用 IMPORT。
    fn pidmasterdata_init(&mut self, i: usize, rest: &str) {
        let Some(target) = self.pidmasterdata_target(i, rest, "INIT") else {
            return;
        };
        let held = match &target {
            Target::Project(dir) => {
                master_data::read_project_file(&dir.join(PROJECT_FILE)).map(|data| data.is_some())
            }
            Target::Drawing => master_data::load_from_drawing(&self.tabs[i].scene.document)
                .transpose()
                .map(|data| data.is_some()),
        };
        match held {
            Ok(false) => {}
            Ok(true) => {
                self.command_line.push_error_coded(
                    MASTERDATA_EXISTS,
                    &format!(
                        "PIDMASTERDATA INIT: {MASTERDATA_EXISTS}: {}里已经有主数据，没有动它。",
                        target_place(&target)
                    ),
                );
                return;
            }
            Err(error) => {
                self.command_line.push_error_coded(
                    error.code(),
                    &format!(
                        "PIDMASTERDATA INIT: {}读不出来，没有动它：{error}",
                        target_place(&target)
                    ),
                );
                return;
            }
        }
        let data = master_data::defaults(self.pid_symbols.current_standard());
        match self.pidmasterdata_write(i, &target, &data) {
            Ok(written) => {
                self.command_line.push_output(&format!(
                    "PIDMASTERDATA: 在{}里建了默认主数据，修订 {}：{}",
                    target_place(&target),
                    written.revision,
                    contents(&written)
                ));
                self.pidmasterdata_warn_if_unused(i, &target);
            }
            Err(error) => self.command_line.push_error_coded(
                error.code(),
                &format!(
                    "PIDMASTERDATA INIT: 没有写进{}：{error}",
                    target_place(&target)
                ),
            ),
        }
    }

    /// EXPORT：把生效的主数据写成一个新文件（§3.1 的格式）。文件已存在就不写。
    fn pidmasterdata_export(&mut self, i: usize, rest: &str) {
        let name = unquoted(rest);
        if name.is_empty() {
            self.command_line
                .push_error("PIDMASTERDATA EXPORT: 写成 PIDMASTERDATA EXPORT <文件>。");
            return;
        }
        let Some((data, source)) = self.pidmasterdata_effective(i, "EXPORT") else {
            return;
        };
        let path = self.pidmasterdata_path(i, name);
        match write_new_file(&path, &format!("{:#}\n", data.to_json())) {
            Ok(()) => self.command_line.push_output(&format!(
                "PIDMASTERDATA: 已把{}的主数据（修订 {}）导出到 {}。",
                place(&source),
                data.revision,
                path.display()
            )),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.command_line.push_error_coded(
                    MASTERDATA_EXISTS,
                    &format!(
                        "PIDMASTERDATA EXPORT: {MASTERDATA_EXISTS}: {} 已经存在，没有覆盖。",
                        path.display()
                    ),
                );
            }
            Err(error) => {
                let error = io_error(&path, &error);
                self.command_line
                    .push_error_coded(error.code(), &format!("PIDMASTERDATA EXPORT: {error}"));
            }
        }
    }

    /// IMPORT：读一个 §3.1 格式的文件，校验过了才整份替换目标位置的主数据。
    fn pidmasterdata_import(&mut self, i: usize, rest: &str) {
        let (name, word) = import_arguments(rest);
        let name = unquoted(name);
        if name.is_empty() {
            self.command_line.push_error(
                "PIDMASTERDATA IMPORT: 写成 PIDMASTERDATA IMPORT <文件> [PROJECT|DRAWING]。",
            );
            return;
        }
        let Some(target) = self.pidmasterdata_target(i, word, "IMPORT") else {
            return;
        };
        let path = self.pidmasterdata_path(i, name);
        let data = match read_file(&path) {
            Ok(data) => data,
            Err(error) => {
                self.command_line.push_error_coded(
                    error.code(),
                    &format!("PIDMASTERDATA IMPORT: {} 不能导入：{error}", path.display()),
                );
                return;
            }
        };
        match self.pidmasterdata_write(i, &target, &data) {
            Ok(written) => {
                self.command_line.push_output(&format!(
                    "PIDMASTERDATA: 已把 {} 导入{}，修订 {}：{}",
                    path.display(),
                    target_place(&target),
                    written.revision,
                    contents(&written)
                ));
                self.pidmasterdata_warn_if_unused(i, &target);
            }
            Err(error) => self.command_line.push_error_coded(
                error.code(),
                &format!(
                    "PIDMASTERDATA IMPORT: 没有写进{}：{error}",
                    target_place(&target)
                ),
            ),
        }
    }

    /// EMBED：把项目文件里生效的主数据原样抄进图内记录，修订号和时间都照抄：图内那份
    /// 是项目文件那一份的副本，单独把这张图发给别人时它自带。图内已经一样就不写，
    /// 免得多出一个什么也没改的撤销步。
    fn pidmasterdata_embed(&mut self, i: usize) {
        let Some((data, source)) = self.pidmasterdata_effective(i, "EMBED") else {
            return;
        };
        let Source::Project(path) = source else {
            self.command_line
                .push_info("PIDMASTERDATA: 生效的就是图内记录，不用再抄。");
            return;
        };
        if master_data::load_from_drawing(&self.tabs[i].scene.document) == Some(Ok(data.clone())) {
            self.command_line.push_info(&format!(
                "PIDMASTERDATA: 图内记录已与项目文件 {} 里的一样（修订 {}），没有改。",
                path.display(),
                data.revision
            ));
            return;
        }
        self.push_undo_snapshot(i, "PIDMASTERDATA");
        master_data::store_in_drawing(&mut self.tabs[i].scene.document, &data);
        self.tabs[i].dirty = true;
        self.command_line.push_output(&format!(
            "PIDMASTERDATA: 已把项目文件 {} 里的主数据（修订 {}）抄进图内记录。",
            path.display(),
            data.revision
        ));
    }

    /// 写到哪里：`word` 是 `PROJECT`、`DRAWING` 或空串。空串时图纸存过是项目文件，否则是
    /// 图内记录。要写项目文件而图纸没存过，或者 `word` 认不得，说明原因并返回 `None`。
    fn pidmasterdata_target(&mut self, i: usize, word: &str, sub: &str) -> Option<Target> {
        let dir = crate::io::drawing_dir(&self.tabs[i].scene.document).map(Path::to_path_buf);
        match (word.to_ascii_uppercase().as_str(), dir) {
            ("" | "PROJECT", Some(dir)) => Some(Target::Project(dir)),
            ("" | "DRAWING", _) => Some(Target::Drawing),
            ("PROJECT", None) => {
                self.command_line.push_error(&format!(
                    "PIDMASTERDATA {sub}: 图纸还没保存，没有目录放项目文件；\
                     先保存，或者写进图内（DRAWING）。"
                ));
                None
            }
            _ => {
                self.command_line.push_error(&format!(
                    "PIDMASTERDATA {sub}: 目标写 PROJECT 或 DRAWING，不认识 {word}。"
                ));
                None
            }
        }
    }

    /// 把 `data` 整份写到目标位置，返回写进去的那份。写图内记录是一个撤销步。
    fn pidmasterdata_write(
        &mut self,
        i: usize,
        target: &Target,
        data: &MasterData,
    ) -> Result<MasterData, MasterDataError> {
        match target {
            Target::Project(dir) => master_data::write_project_file(dir, data, None),
            Target::Drawing => {
                self.push_undo_snapshot(i, "PIDMASTERDATA");
                let written = master_data::write_drawing_record(
                    &mut self.tabs[i].scene.document,
                    data,
                    None,
                )?;
                self.tabs[i].dirty = true;
                Ok(written)
            }
        }
    }

    /// 刚写进图内记录的主数据不生效（目录里的项目文件有主数据）时，提醒一句。
    fn pidmasterdata_warn_if_unused(&mut self, i: usize, target: &Target) {
        if !matches!(target, Target::Drawing) {
            return;
        }
        let document = &self.tabs[i].scene.document;
        if let Resolved::Configured {
            source: Source::Project(path),
            ..
        }
        | Resolved::Unreadable {
            source: Source::Project(path),
            ..
        } = master_data::resolve(document, crate::io::drawing_dir(document))
        {
            self.command_line.push_warning(&format!(
                "PIDMASTERDATA: 图内这份不生效，项目文件 {} 优先。",
                path.display()
            ));
        }
    }

    /// 生效的主数据和它在哪里；未配置或读不出来时说明原因，返回 `None`。
    fn pidmasterdata_effective(&mut self, i: usize, sub: &str) -> Option<(MasterData, Source)> {
        let document = &self.tabs[i].scene.document;
        match master_data::resolve(document, crate::io::drawing_dir(document)) {
            Resolved::Configured { data, source } => Some((data, source)),
            Resolved::Unreadable { source, error } => {
                self.command_line.push_error_coded(
                    error.code(),
                    &format!("PIDMASTERDATA {sub}: {}读不出来：{error}", place(&source)),
                );
                None
            }
            Resolved::NotConfigured => {
                self.command_line.push_error_coded(
                    MASTERDATA_NOT_CONFIGURED,
                    &format!(
                        "PIDMASTERDATA {sub}: {MASTERDATA_NOT_CONFIGURED}: 未配置主数据；\
                         先用 PIDMASTERDATA INIT 建一份，或用 IMPORT 导入。"
                    ),
                );
                None
            }
        }
    }

    /// 命令行上的文件名：不是完整路径时从图纸所在目录算起，没存过的图从当前目录算起。
    fn pidmasterdata_path(&self, i: usize, name: &str) -> PathBuf {
        let path = Path::new(name);
        match crate::io::drawing_dir(&self.tabs[i].scene.document) {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path.to_path_buf(),
        }
    }
}

/// 「项目文件 <路径>」或「图内记录」。
fn place(source: &Source) -> String {
    match source {
        Source::Project(path) => format!("项目文件 {}", path.display()),
        Source::Drawing => "图内记录".to_owned(),
    }
}

fn target_place(target: &Target) -> String {
    match target {
        Target::Project(dir) => place(&Source::Project(dir.join(PROJECT_FILE))),
        Target::Drawing => place(&Source::Drawing),
    }
}

/// 各表多少条，和图例标准。
fn contents(data: &MasterData) -> String {
    let hidden = data
        .nominal_diameters
        .iter()
        .filter(|diameter| diameter.hidden)
        .count();
    format!(
        "公称直径 {} 个（隐藏 {hidden} 个），管道等级 {} 条，绝热等级 {} 条，流体介质 {} 条，\
         PBS 节点 {} 个；图例标准：{}。",
        data.nominal_diameters.len(),
        data.pipe_classes.len(),
        data.insulation_classes.len(),
        data.fluid_codes.len(),
        data.pbs.len(),
        data.legend_standard.as_deref().unwrap_or("未锁定"),
    )
}

/// `text` 的第一个词，和它后面剩下的部分。
fn first_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (&text[..end], text[end..].trim_start())
}

/// IMPORT 的文件名，和跟在它后面的目标词（没有是空串）。只有一个词时那就是文件名。
fn import_arguments(rest: &str) -> (&str, &str) {
    let rest = rest.trim();
    match rest.rsplit_once(char::is_whitespace) {
        Some((name, word))
            if word.eq_ignore_ascii_case("PROJECT") || word.eq_ignore_ascii_case("DRAWING") =>
        {
            (name.trim_end(), word)
        }
        _ => (rest, ""),
    }
}

/// 去掉首尾空白和成对的双引号。
fn unquoted(name: &str) -> &str {
    let name = name.trim();
    name.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(name)
}

/// 读一个 §3.1 格式的主数据文件并校验；开头的 UTF-8 BOM 不算错。
fn read_file(path: &Path) -> Result<MasterData, MasterDataError> {
    let bytes = fs::read(path).map_err(|error| io_error(path, &error))?;
    let text = bytes
        .strip_prefix(b"\xEF\xBB\xBF")
        .unwrap_or(bytes.as_slice());
    let value: serde_json::Value =
        serde_json::from_slice(text).map_err(|error| MasterDataError::Schema {
            at: String::new(),
            reason: format!("不是合法的 JSON：{error}"),
        })?;
    master_data::validate(&value)
}

/// 新建 `path` 写进 `text`。已经存在是 `AlreadyExists`，不动它；写到一半出错就删掉写了
/// 一半的文件。
fn write_new_file(path: &Path, text: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let written = file.write_all(text.as_bytes());
    drop(file);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written
}

fn io_error(path: &Path, error: &io::Error) -> MasterDataError {
    MasterDataError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 测试用的临时目录，用完删掉。
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "ocs-pidmasterdata-{}-{}-{nanos}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("create the scratch folder");
            Self(dir)
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

    /// 一张新图；给了 `dir` 就当它已存进那个目录。
    fn app_with_drawing(dir: Option<&Path>) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path =
            dir.map(|dir| dir.join("PID-1101.dwg").to_string_lossy().into_owned());
        app
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

    fn drawing_record(app: &OpenCADStudio) -> Option<Result<MasterData, MasterDataError>> {
        master_data::load_from_drawing(&app.tabs[app.active_tab].scene.document)
    }

    fn undo_steps(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    /// INIT → STATUS → EXPORT → 改文件 → IMPORT 一条龙。图纸存过，主数据在项目文件里；
    /// 目标已有主数据的 INIT、目标已存在的 EXPORT、校验不过的 IMPORT 都什么也不写。
    #[test]
    fn pidmasterdata_init_status_export_edit_import() {
        let scratch = Scratch::new();
        let mut app = app_with_drawing(Some(&scratch.0));
        let project = scratch.path(PROJECT_FILE);

        let ran = run(&mut app, "PIDMASTERDATA INIT");
        assert_eq!(ran.error, None, "{}", ran.said);
        let made = master_data::read_project_file(&project)
            .expect("the project file reads")
            .expect("INIT put master data in the project file");
        assert_eq!(made.revision, 1);
        assert_eq!(made.nominal_diameters.len(), 19);
        assert!(made.pipe_classes.is_empty());
        assert_eq!(
            made.legend_standard.as_deref(),
            Some(app.pid_symbols.current_standard())
        );
        assert!(
            ran.said.contains(&project.display().to_string()),
            "{}",
            ran.said
        );
        assert_eq!(
            drawing_record(&app),
            None,
            "the drawing record is left alone"
        );

        let before = fs::read(&project).unwrap();
        let again = run(&mut app, "PIDMASTERDATA INIT");
        assert_eq!(
            again.code.as_deref(),
            Some(MASTERDATA_EXISTS),
            "{}",
            again.said
        );
        assert_eq!(fs::read(&project).unwrap(), before, "nothing written");

        let status = run(&mut app, "PIDMASTERDATA STATUS");
        assert_eq!(status.error, None, "{}", status.said);
        for part in ["项目文件", "修订 1", "公称直径 19 个", "管道等级 0 条"] {
            assert!(status.said.contains(part), "{part}: {}", status.said);
        }
        let bare = run(&mut app, "PIDMASTERDATA");
        assert!(
            bare.said.contains("修订 1") && bare.said.contains("公称直径 19 个"),
            "the bare verb is STATUS: {}",
            bare.said
        );

        let ran = run(&mut app, "PIDMASTERDATA EXPORT md.json");
        assert_eq!(ran.error, None, "{}", ran.said);
        let exported = scratch.path("md.json");
        let text = fs::read_to_string(&exported).expect("exported next to the drawing");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(master_data::validate(&value), Ok(made.clone()));
        let again = run(&mut app, "PIDMASTERDATA EXPORT md.json");
        assert_eq!(
            again.code.as_deref(),
            Some(MASTERDATA_EXISTS),
            "{}",
            again.said
        );
        assert_eq!(
            fs::read_to_string(&exported).unwrap(),
            text,
            "not overwritten"
        );

        let mut edited = value;
        edited["pipe_classes"] =
            serde_json::json!([{ "name": "A1A", "attrs": { "压力等级": "PN16" } }]);
        fs::write(&exported, serde_json::to_string_pretty(&edited).unwrap()).unwrap();
        let ran = run(&mut app, "PIDMASTERDATA IMPORT md.json");
        assert_eq!(ran.error, None, "{}", ran.said);
        let imported = master_data::read_project_file(&project).unwrap().unwrap();
        assert_eq!(imported.revision, 2);
        assert_eq!(imported.pipe_classes.len(), 1);
        assert_eq!(imported.pipe_classes[0].name, "A1A");
        assert_eq!(imported.pipe_classes[0].attr("压力等级"), Some("PN16"));
        let status = run(&mut app, "PIDMASTERDATA STATUS");
        for part in ["修订 2", "管道等级 1 条"] {
            assert!(status.said.contains(part), "{part}: {}", status.said);
        }

        let before = fs::read(&project).unwrap();
        edited["pipe_classes"] = serde_json::json!([{ "name": "A1A" }, { "name": "A1A" }]);
        fs::write(&exported, serde_json::to_string_pretty(&edited).unwrap()).unwrap();
        let refused = run(&mut app, "PIDMASTERDATA IMPORT md.json");
        assert_eq!(
            refused.code.as_deref(),
            Some("MASTERDATA_DUPLICATE"),
            "{}",
            refused.said
        );
        assert!(
            refused
                .error
                .is_some_and(|error| error.contains("pipe_classes[1].name")),
            "{}",
            refused.said
        );
        assert_eq!(fs::read(&project).unwrap(), before, "nothing imported");
    }

    /// 没存过的图只有图内记录：INIT 写进图内，是一个撤销步，撤销后图内没有这条记录；
    /// 要写项目文件就说图纸还没保存。
    #[test]
    fn pidmasterdata_on_an_unsaved_drawing_writes_the_drawing_record_in_one_undo_step() {
        let mut app = app_with_drawing(None);
        let steps = undo_steps(&app);

        let ran = run(&mut app, "PIDMASTERDATA INIT");
        assert_eq!(ran.error, None, "{}", ran.said);
        let Some(Ok(made)) = drawing_record(&app) else {
            panic!("INIT wrote the drawing record: {}", ran.said);
        };
        assert_eq!((made.revision, made.nominal_diameters.len()), (1, 19));
        assert!(made.updated_at > 0);
        assert_eq!(undo_steps(&app), steps + 1);
        assert!(app.tabs[app.active_tab].dirty);

        let again = run(&mut app, "PIDMASTERDATA INIT DRAWING");
        assert_eq!(
            again.code.as_deref(),
            Some(MASTERDATA_EXISTS),
            "{}",
            again.said
        );
        assert_eq!(undo_steps(&app), steps + 1, "a refused INIT is no step");

        let status = run(&mut app, "PIDMASTERDATA STATUS");
        assert!(
            status.said.contains("图内记录") && status.said.contains("修订 1"),
            "{}",
            status.said
        );

        let _ = app.update(Message::Undo);
        assert_eq!(drawing_record(&app), None, "undo takes the record away");

        let refused = run(&mut app, "PIDMASTERDATA INIT PROJECT");
        assert!(
            refused.error.is_some_and(|error| error.contains("保存")),
            "{}",
            refused.said
        );
        assert_eq!(drawing_record(&app), None);
    }

    /// IMPORT 到图内记录，修订号接着图内原有的数。EMBED 把项目文件里的那份原样抄进图内，
    /// 图内那份没在用，STATUS 这么说；一步撤销。未配置时 EMBED、EXPORT 都拒绝。
    #[test]
    fn pidmasterdata_import_into_the_drawing_and_embed() {
        let scratch = Scratch::new();
        let source = scratch.path("source.json");
        let defaults = master_data::defaults("内置示例标准");
        fs::write(&source, format!("{:#}", defaults.to_json())).unwrap();

        let mut app = app_with_drawing(None);
        let line = format!("PIDMASTERDATA IMPORT \"{}\" DRAWING", source.display());
        let ran = run(&mut app, &line);
        assert_eq!(ran.error, None, "{}", ran.said);
        let ran = run(&mut app, &line);
        assert_eq!(ran.error, None, "{}", ran.said);
        let Some(Ok(record)) = drawing_record(&app) else {
            panic!("IMPORT wrote the drawing record: {}", ran.said);
        };
        assert_eq!(record.revision, 2);

        let mut app = app_with_drawing(Some(&scratch.0));
        let embed = run(&mut app, "PIDMASTERDATA EMBED");
        assert_eq!(
            embed.code.as_deref(),
            Some(MASTERDATA_NOT_CONFIGURED),
            "{}",
            embed.said
        );
        let export = run(&mut app, "PIDMASTERDATA EXPORT out.json");
        assert_eq!(
            export.code.as_deref(),
            Some(MASTERDATA_NOT_CONFIGURED),
            "{}",
            export.said
        );
        assert!(!scratch.path("out.json").exists());

        let ran = run(&mut app, "PIDMASTERDATA INIT");
        assert_eq!(ran.error, None, "{}", ran.said);
        let project = master_data::read_project_file(&scratch.path(PROJECT_FILE))
            .unwrap()
            .unwrap();
        let steps = undo_steps(&app);
        let ran = run(&mut app, "PIDMASTERDATA EMBED");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(drawing_record(&app), Some(Ok(project)), "copied as it is");
        assert_eq!(undo_steps(&app), steps + 1);
        let status = run(&mut app, "PIDMASTERDATA STATUS");
        assert!(status.said.contains("没在用"), "{}", status.said);

        let _ = app.update(Message::Undo);
        assert_eq!(drawing_record(&app), None);
    }

    /// 只读会话里 INIT、IMPORT、EMBED 被拒，什么也没写；STATUS、EXPORT 照常。
    #[test]
    fn pidmasterdata_in_a_readonly_session_reads_but_does_not_write() {
        let scratch = Scratch::new();
        let mut app = app_with_drawing(Some(&scratch.0));
        let ran = run(&mut app, "PIDMASTERDATA INIT");
        assert_eq!(ran.error, None, "{}", ran.said);
        let ran = run(&mut app, "PIDMASTERDATA EXPORT md.json");
        assert_eq!(ran.error, None, "{}", ran.said);
        let project = scratch.path(PROJECT_FILE);
        let before = fs::read(&project).unwrap();
        let steps = undo_steps(&app);

        app.collab.readonly = true;
        for line in [
            "PIDMASTERDATA INIT",
            "PIDMASTERDATA INIT DRAWING",
            "PIDMASTERDATA IMPORT md.json",
            "PIDMASTERDATA IMPORT md.json DRAWING",
            "PIDMASTERDATA EMBED",
            "pidmasterdata embed",
        ] {
            let ran = run(&mut app, line);
            assert!(ran.code.is_some(), "{line}: refused with a code");
            assert!(
                ran.error.is_some_and(|error| error.contains("只读")),
                "{line}: {}",
                ran.said
            );
        }
        assert_eq!(fs::read(&project).unwrap(), before);
        assert_eq!(drawing_record(&app), None);
        assert_eq!(undo_steps(&app), steps);

        let status = run(&mut app, "PIDMASTERDATA STATUS");
        assert_eq!(status.error, None, "{}", status.said);
        assert!(status.said.contains("修订 1"), "{}", status.said);
        let bare = run(&mut app, "PIDMASTERDATA");
        assert_eq!(bare.error, None, "{}", bare.said);
        let export = run(&mut app, "PIDMASTERDATA EXPORT again.json");
        assert_eq!(export.error, None, "{}", export.said);
        assert!(scratch.path("again.json").exists());
    }

    /// 命令行上的 `PIDMASTERDATA INIT DRAWING` 整行是一条命令，只跑一次：不先拿裸动词探一遍
    /// （那会先说一遍 STATUS）。
    #[test]
    fn pidmasterdata_from_the_command_line_runs_once() {
        let mut app = app_with_drawing(None);
        let steps = undo_steps(&app);
        let ran = run(&mut app, "PIDMASTERDATA INIT DRAWING");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(undo_steps(&app), steps + 1);
        assert!(
            !ran.said.contains("未配置"),
            "STATUS ran first: {}",
            ran.said
        );
        assert_eq!(ran.said.matches("建了").count(), 1, "{}", ran.said);
    }

    #[test]
    fn pidmasterdata_is_registered_and_says_how_it_is_written() {
        assert!(crate::command::all_registered_command_names().contains(&"PIDMASTERDATA"));
        let mut app = app_with_drawing(None);
        for line in [
            "PIDMASTERDATA FROB",
            "PIDMASTERDATA EXPORT",
            "PIDMASTERDATA IMPORT",
            "PIDMASTERDATA INIT SOMEWHERE",
        ] {
            let ran = run(&mut app, line);
            assert!(
                ran.error
                    .is_some_and(|error| error.starts_with("PIDMASTERDATA")),
                "{line}: {}",
                ran.said
            );
        }
        assert_eq!(drawing_record(&app), None);
    }
}
