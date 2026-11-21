//! PIDINDEX、PIDLOCATE：项目索引，按位号定位（`pid::project_index`，
//! `SPEC-pid-project-index.md` §4、§5.2）。
//!
//! ```text
//! PIDINDEX              几张图（几张读不出来）、跨图重号几处、连接符配成几对、几个没配上
//! PIDINDEX DUPLICATES   逐条列出跨图重号：类型、位号、各在哪张图的哪里
//! PIDINDEX LINKS        逐条列出连接符：配成对的，和没配上的及其 status
//! PIDINDEX CANCEL       不再读下一个文件；读好的记着，下次扫接着用
//! PIDLOCATE <位号> [<图号>]
//!                       选中这个位号的对象、缩放到它；在别的图上就先打开那张、切过去
//! ```
//!
//! 项目目录是当前图所在的目录（§3.7），当前图没保存过就没有项目目录
//! （`INDEX_NO_PROJECT_DIR`）。目录里的图正在程序里打开着的，摘要它内存里的那份，含没
//! 保存的修改，每次现做；没打开的读磁盘文件，摘要按「路径 + 大小 + 修改时间」记在内存
//! 里（[`SummaryCache`]），文件没变就不再读。不往项目目录里写任何东西（§4）。
//!
//! 要从磁盘读的图在后台线程里读、做摘要（§4，`read_in_background`），界面线程只收
//! 摘要：命令先说要读几个，约每读完十分之一报一次进度，读完接着说。读着的时候再发
//! `PIDINDEX`，排在后面、读完一起说；`PIDINDEX CANCEL` 不再读下一个文件，读回来的照样
//! 记进缓存。缓存里的都没变就不起后台，当场说。列图、建索引出错时连同错误码一起说
//! （`INDEX_DRAWING_ID_DUPLICATED`、项目文件的 `MASTERDATA_...`、`INDEX_IO_ERROR`），
//! 命令行的错误码也设上。几个命令都不改图，只读会话里照常能用。
//!
//! `PIDLOCATE` 不给图号时先在当前图上找，有就不扫项目；没有就扫项目（同上，要读盘就
//! 在后台读，读完接着找）：只有一张图上有就去那一张，几张图上都有就逐处列出来
//! （`LOCATE_AMBIGUOUS`），哪张都没有是 `LOCATE_NOT_FOUND`。给了图号就扫项目、只看那一张。
//! 去一张图：有标签页开着它就切过去，没有就打开，打开以后接着找。在哪张图上都按
//! （类型编码, 位号）照图现在的样子现找，不用摘要里记的句柄（§5.2）；位号不分大小写，
//! 与 `PIDTAG` 一样。找到的只选中它们、缩放到它们；画不出样子的按外框和定位点缩放。

use super::*;
use crate::pid::project_index::{
    build_index, list_drawings, summarize, DrawingFile, DrawingSummary, FailedDrawing, Fingerprint,
    IndexError, LinkEnd, LinkFamily, LinkStatus, Place, ProjectIndex, SummaryMeta, SummaryObject,
    SummarySource, DEFAULT_TOLERANCE,
};
use crate::pid::symbols::SymbolLibrary;
use crate::pid::topology::parse_handle;
use document::Document;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 当前图没保存过，没有项目目录（§5.3）。
pub(in crate::app) const INDEX_NO_PROJECT_DIR: &str = "INDEX_NO_PROJECT_DIR";

/// 要定位的位号哪张图上都没有（§5.3）。
pub(in crate::app) const LOCATE_NOT_FOUND: &str = "LOCATE_NOT_FOUND";

/// 要定位的位号在几张图上都有，又没给图号（§5.3）。
pub(in crate::app) const LOCATE_AMBIGUOUS: &str = "LOCATE_AMBIGUOUS";

/// 按外框和定位点缩放时四边至少留出的宽度，图纸单位。
const FRAME_MARGIN: f64 = 10.0;

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
        self.remember(drawing.path.clone(), size, modified, summary.clone());
        Ok(summary)
    }

    /// 记下一张图的摘要。取不到修改时间的不记：认不出文件变没变，下次照样重读。
    fn remember(
        &mut self,
        path: PathBuf,
        size: u64,
        modified: Option<SystemTime>,
        summary: DrawingSummary,
    ) {
        match modified {
            Some(modified) => {
                let cached = Cached {
                    size,
                    modified,
                    summary,
                };
                self.entries.insert(path, cached);
            }
            None => {
                self.entries.remove(&path);
            }
        }
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

/// 读一个文件的办法：命令用 [`crate::io::load_file`]，测试换成数次数的、卡着等放行的。在
/// 后台线程里调。
type Loader = Arc<dyn Fn(&Path) -> Result<Document, String> + Send + Sync>;

/// 项目索引在程序里的状态（§4）：磁盘上的图的摘要缓存，和正在后台读文件的那次
/// 扫描。属于程序、不属于哪个标签页：几个标签页可以是同一个项目。
pub(in crate::app) struct PidIndex {
    cache: SummaryCache,
    /// 正在后台读文件的扫描；`None` 时没在扫。
    scan: Option<Scan>,
    /// 下一次扫描的编号：后台读回来的消息凭它认是哪一次的。
    next_scan: u64,
    loader: Loader,
    /// 为定位正在打开的图（`PIDLOCATE`），打开以后接着找。只等最近的一张；按那次打开
    /// 认，不按文件：没打开成的，以后再打开这个文件也不接着找。
    opening: Option<Opening>,
}

impl Default for PidIndex {
    fn default() -> Self {
        Self {
            cache: SummaryCache::default(),
            scan: None,
            next_scan: 0,
            loader: Arc::new(crate::io::load_file),
            opening: None,
        }
    }
}

/// 一次后台扫描：要读几个文件、读回来几个，读不出来的为什么，扫完接着做什么。
struct Scan {
    id: u64,
    project_dir: PathBuf,
    total: usize,
    done: usize,
    /// 立起来就不再读下一个文件（`PIDINDEX CANCEL`）。
    cancel: Arc<AtomicBool>,
    /// 后台读不出来的文件和原因：扫完建索引时照这个说，不在界面线程里再读一遍。
    failed: HashMap<PathBuf, String>,
    /// 扫完接着做的，按命令的先后。
    then: Vec<Then>,
}

/// 扫完接着做的事：`PIDINDEX` 的三种说法，或者 `PIDLOCATE` 接着找。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Then {
    Overview,
    Duplicates,
    Links,
    Locate(Locate),
}

impl Then {
    /// 说话时打头的命令。
    fn verb(&self) -> &'static str {
        match self {
            Self::Overview | Self::Duplicates | Self::Links => "PIDINDEX",
            Self::Locate(_) => "PIDLOCATE",
        }
    }
}

/// `PIDLOCATE` 要找的：位号，和给了的图号。
#[derive(Clone, Debug, PartialEq, Eq)]
struct Locate {
    tag: String,
    drawing: Option<String>,
}

/// 在一张图上现找的对象：位号不分大小写；类型编码是 `None` 时不论类型。
struct Wanted {
    tag: String,
    type_code: Option<String>,
}

/// 为定位打开的一张图：编号是 `open_id` 的那次打开（`OpenProgress::id`）做完了，就在
/// 它上面找 `wanted`。
struct Opening {
    open_id: u64,
    drawing_id: String,
    wanted: Wanted,
}

/// 后台读回来的一条（`Message::PidIndexRead`）。
#[derive(Debug, Clone)]
pub enum Read {
    /// 读了一个文件。
    File(Box<FileRead>),
    /// 这次扫描的文件读完了，或者取消以后不再读了。
    Done,
}

/// 读一个文件读到的：读之前文件的大小和修改时间，读成的摘要或读不出来的原因。
#[derive(Debug, Clone)]
pub struct FileRead {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    summary: Result<DrawingSummary, String>,
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

/// 在后台线程里依次读 `files`、做成摘要，读一个交回一个（§4）。`cancel` 立起来就不再读
/// 下一个；最后交回 [`Read::Done`]，线程意外退出了也交。
fn read_in_background(
    files: Vec<PathBuf>,
    library: SymbolLibrary,
    load: Loader,
    cancel: Arc<AtomicBool>,
) -> impl iced::futures::Stream<Item = Read> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            for path in files {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let read = Read::File(Box::new(read_one(path, &library, &*load)));
                if tx.unbounded_send(read).is_err() {
                    return;
                }
            }
        });
        iced::futures::StreamExt::chain(rx, iced::futures::stream::iter([Read::Done]))
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(cancel);
        let reads: Vec<Read> = files
            .into_iter()
            .map(|path| Read::File(Box::new(read_one(path, &library, &*load))))
            .chain([Read::Done])
            .collect();
        iced::futures::stream::iter(reads)
    }
}

/// 读一个文件、做成摘要。图号先按文件名填、PBS 节点先空着，从缓存取用时换成项目文件
/// 现在写的（[`SummaryCache::summary`]）。读文件或做摘要时崩了，只算这个文件读不出来。
fn read_one(
    path: PathBuf,
    library: &SymbolLibrary,
    load: &dyn Fn(&Path) -> Result<Document, String>,
) -> FileRead {
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return FileRead {
                path,
                size: 0,
                modified: None,
                summary: Err(error.to_string()),
            };
        }
    };
    let size = metadata.len();
    let modified = metadata.modified().ok();
    let drawing = DrawingFile {
        file: path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        drawing_id: path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
        path,
        pbs: None,
    };
    let fingerprint = modified.map(|modified| Fingerprint {
        size,
        modified_at: unix_seconds(modified),
    });
    let summary = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<DrawingSummary, String> {
            let document = load(&drawing.path)?;
            Ok(summarize(
                &document,
                library,
                meta(&drawing, SummarySource::File, fingerprint),
                DEFAULT_TOLERANCE,
            ))
        },
    ))
    .unwrap_or_else(|payload| Err(panicked(payload.as_ref())));
    FileRead {
        path: drawing.path,
        size,
        modified,
        summary,
    }
}

/// 崩溃时带出来的话，当这个文件读不出来的原因。
fn panicked(payload: &(dyn std::any::Any + Send)) -> String {
    let said = payload
        .downcast_ref::<&str>()
        .map(|said| (*said).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned());
    match said {
        Some(said) => format!("读的时候程序出了错：{said}"),
        None => "读的时候程序出了错".to_string(),
    }
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
        let then = match option.as_str() {
            "" => Some(Then::Overview),
            "DUPLICATES" => Some(Then::Duplicates),
            "LINKS" => Some(Then::Links),
            "CANCEL" => None,
            _ => {
                self.command_line.push_error(&format!(
                    "PIDINDEX: 不认识 {option}；写 DUPLICATES、LINKS 或 CANCEL，或者只写 PIDINDEX。"
                ));
                return Some(Task::none());
            }
        };
        if !rest.is_empty() {
            self.command_line.push_error(&format!(
                "PIDINDEX {option}: 后面不用再写东西，多了 {}。",
                rest.join(" ")
            ));
            return Some(Task::none());
        }
        let Some(then) = then else {
            self.pidindex_cancel();
            return Some(Task::none());
        };
        Some(self.pidindex_start(i, then))
    }

    /// 扫当前图所在的项目目录，扫完做 `then`。缓存里的都没变就当场扫完、当场说；有要从
    /// 磁盘读的挪到后台读（§4），这里先交差，读完接着说。已经在扫同一个目录就排在它后面。
    /// 没有项目目录、列图或建索引出错时在命令行连同错误码说。
    fn pidindex_start(&mut self, i: usize, then: Then) -> Task<Message> {
        let verb = then.verb();
        let Some(project_dir) =
            crate::io::drawing_dir(&self.tabs[i].scene.document).map(Path::to_path_buf)
        else {
            self.command_line.push_error_coded(
                INDEX_NO_PROJECT_DIR,
                &format!(
                    "{verb}: {INDEX_NO_PROJECT_DIR}: 当前图没保存过，不属于哪个项目；存盘后再扫。"
                ),
            );
            return Task::none();
        };
        if let Some(scan) = &mut self.pid_index.scan {
            if same_file(&scan.project_dir) == same_file(&project_dir) {
                scan.then.push(then);
                let said = format!(
                    "{verb}: 还在读，{}/{} 个文件；读完接着说。",
                    scan.done, scan.total
                );
                self.command_line.push_info(&said);
            } else {
                let said = format!(
                    "{verb}: 正在扫 {}；等它扫完，或者 PIDINDEX CANCEL 以后再扫这里。",
                    scan.project_dir.display()
                );
                self.command_line.push_error(&said);
            }
            return Task::none();
        }
        let mut unread = Vec::new();
        let scanned = self.pidindex_scan(&project_dir, &mut |path| {
            unread.push(path.to_path_buf());
            Err(String::new())
        });
        match scanned {
            Ok((index, summaries)) if unread.is_empty() => {
                self.pidindex_report(&project_dir, &index, &summaries, then)
            }
            Ok(_) => self.pidindex_read(project_dir, unread, then),
            Err(error) => {
                self.command_line
                    .push_error_coded(error.code(), &format!("{verb}: {error}"));
                Task::none()
            }
        }
    }

    /// 用 `load` 读缓存里没有、或者变了的图，扫一遍 `project_dir`，得出索引和读得出来的
    /// 各图的摘要。打开着的图以标签页图纸的 `source_path` 认，与定项目目录用的是同一个。
    fn pidindex_scan(
        &mut self,
        project_dir: &Path,
        load: &mut dyn FnMut(&Path) -> Result<Document, String>,
    ) -> Result<(ProjectIndex, Vec<DrawingSummary>), IndexError> {
        let open: Vec<(&Path, &Document)> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                let document = &tab.scene.document;
                Some((Path::new(document.source_path.as_deref()?), document))
            })
            .collect();
        scan(
            project_dir,
            &open,
            &self.pid_symbols,
            &mut self.pid_index.cache,
            load,
        )
    }

    /// 在后台读 `files`（§4）：先说要读几个；读回来的由 `on_pid_index_read` 收。
    fn pidindex_read(
        &mut self,
        project_dir: PathBuf,
        files: Vec<PathBuf>,
        then: Then,
    ) -> Task<Message> {
        let id = self.pid_index.next_scan;
        self.pid_index.next_scan += 1;
        let total = files.len();
        let cancel = Arc::new(AtomicBool::new(false));
        self.command_line.push_info(&format!(
            "{}: 要从磁盘读 {total} 个文件，在后台读，读完接着说；PIDINDEX CANCEL 可以取消。",
            then.verb()
        ));
        self.pid_index.scan = Some(Scan {
            id,
            project_dir,
            total,
            done: 0,
            cancel: Arc::clone(&cancel),
            failed: HashMap::new(),
            then: vec![then],
        });
        let reads = read_in_background(
            files,
            self.pid_symbols.clone(),
            Arc::clone(&self.pid_index.loader),
            cancel,
        );
        Task::run(reads, move |read| Message::PidIndexRead(id, read))
    }

    /// 后台读回来的一条。读到的摘要记进缓存，取消了的扫描读回来的也记；是这次扫描的就数上，
    /// 约每读完十分之一报一次进度，读完了建索引、接着说；接着做的要打开图（`PIDLOCATE`）就
    /// 交出打开它的任务。
    pub(in crate::app) fn on_pid_index_read(&mut self, scan: u64, read: Read) -> Task<Message> {
        let current = self
            .pid_index
            .scan
            .as_ref()
            .is_some_and(|running| running.id == scan);
        let Read::File(file) = read else {
            return if current {
                self.pidindex_finish()
            } else {
                Task::none()
            };
        };
        let FileRead {
            path,
            size,
            modified,
            summary,
        } = *file;
        let failed = match summary {
            Ok(summary) => {
                self.pid_index.cache.remember(path, size, modified, summary);
                None
            }
            Err(why) => {
                self.pid_index.cache.entries.remove(&path);
                Some((path, why))
            }
        };
        let Some(running) = self.pid_index.scan.as_mut().filter(|_| current) else {
            return Task::none();
        };
        running.done += 1;
        running.failed.extend(failed);
        let (done, total) = (running.done, running.total);
        let verb = running.then.first().map_or("PIDINDEX", Then::verb);
        if done < total && done % (total / 10).max(1) == 0 {
            self.command_line
                .push_info(&format!("{verb}: 读了 {done}/{total} 个文件。"));
        }
        Task::none()
    }

    /// 这次扫描要读的都读回来了：用缓存建索引，后台读不出来的照它说的原因记，再按先后做
    /// 排着的。界面线程不读文件：缓存里还没有的，是读完以后又变了的。
    fn pidindex_finish(&mut self) -> Task<Message> {
        let Some(Scan {
            project_dir,
            failed,
            then,
            ..
        }) = self.pid_index.scan.take()
        else {
            return Task::none();
        };
        let scanned = self.pidindex_scan(&project_dir, &mut |path| {
            Err(failed
                .get(path)
                .cloned()
                .unwrap_or_else(|| "读完以后文件又变了，这次没算上，再扫一次".to_string()))
        });
        match scanned {
            Ok((index, summaries)) => {
                let mut tasks = Vec::with_capacity(then.len());
                for then in then {
                    tasks.push(self.pidindex_report(&project_dir, &index, &summaries, then));
                }
                Task::batch(tasks)
            }
            Err(error) => {
                let mut said = Vec::new();
                for verb in then.iter().map(Then::verb) {
                    if !said.contains(&verb) {
                        said.push(verb);
                        self.command_line
                            .push_error_coded(error.code(), &format!("{verb}: {error}"));
                    }
                }
                Task::none()
            }
        }
    }

    /// 做扫完接着做的一件。定位要打开图时交出打开它的任务。
    fn pidindex_report(
        &mut self,
        project_dir: &Path,
        index: &ProjectIndex,
        summaries: &[DrawingSummary],
        then: Then,
    ) -> Task<Message> {
        match then {
            Then::Overview => self.pidindex_overview(index),
            Then::Duplicates => self.pidindex_duplicates(index),
            Then::Links => self.pidindex_links(index),
            Then::Locate(locate) => {
                return self.pidlocate_found(project_dir, index, summaries, locate);
            }
        }
        Task::none()
    }

    /// `PIDINDEX CANCEL`：后台不再读下一个文件，排着的不做了。正读着的那个读回来照样记进
    /// 缓存，下次扫接着用。
    fn pidindex_cancel(&mut self) {
        let Some(scan) = self.pid_index.scan.take() else {
            self.command_line.push_info("PIDINDEX CANCEL: 没有在扫。");
            return;
        };
        scan.cancel.store(true, Ordering::Relaxed);
        self.command_line.push_output(&format!(
            "PIDINDEX CANCEL: 取消了，{} 个文件读了 {} 个；读好的记着，下次扫接着用。",
            scan.total, scan.done
        ));
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

    // ── PIDLOCATE ──

    pub(super) fn dispatch_pidlocate(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let mut words = cmd.split_whitespace();
        if !words
            .next()
            .is_some_and(|verb| verb.eq_ignore_ascii_case("PIDLOCATE"))
        {
            return None;
        }
        let Some(tag) = words.next() else {
            self.command_line
                .push_error("PIDLOCATE: 要写位号：PIDLOCATE <位号> [<图号>]。");
            return Some(Task::none());
        };
        let drawing = words.next().map(str::to_string);
        let rest: Vec<&str> = words.collect();
        if !rest.is_empty() {
            self.command_line.push_error(&format!(
                "PIDLOCATE {tag}: 位号和图号后面不用再写东西，多了 {}。",
                rest.join(" ")
            ));
            return Some(Task::none());
        }
        let tag = tag.to_string();
        if drawing.is_none() {
            let here = Wanted {
                tag: tag.clone(),
                type_code: None,
            };
            if self.pidlocate_in(i, &here, "当前图上") {
                return Some(Task::none());
            }
        }
        Some(self.pidindex_start(i, Then::Locate(Locate { tag, drawing })))
    }

    /// 扫完以后接着找 `locate`：项目里哪几张图上有（`ProjectIndex::places`），给了图号就
    /// 只看那一张。一张都没有、好几张都有时说出来；只有一张就去那一张，那张图上同位号的
    /// 都选中：图号分不开它们。
    fn pidlocate_found(
        &mut self,
        project_dir: &Path,
        index: &ProjectIndex,
        summaries: &[DrawingSummary],
        locate: Locate,
    ) -> Task<Message> {
        let Locate { tag, drawing } = locate;
        // 索引按位号原样查，可位号不分大小写：项目里写成什么样就照什么样查。
        let spellings: BTreeSet<&str> = summaries
            .iter()
            .flat_map(|summary| &summary.objects)
            .filter(|object| object.own_tag && object.tag.eq_ignore_ascii_case(&tag))
            .map(|object| object.tag.as_str())
            .collect();
        let places: Vec<Place> = spellings
            .into_iter()
            .flat_map(|spelled| index.places(summaries, None, spelled))
            .filter(|place| {
                drawing
                    .as_deref()
                    .is_none_or(|id| place.drawing_id.eq_ignore_ascii_case(id))
            })
            .collect();
        let mut drawings: Vec<&str> = places
            .iter()
            .map(|place| place.drawing_id.as_str())
            .collect();
        drawings.sort_unstable();
        drawings.dedup();
        match drawings[..] {
            [] => {
                self.pidlocate_not_found(index, &tag, drawing.as_deref());
                Task::none()
            }
            [drawing_id] => {
                let wanted = Wanted {
                    type_code: type_code_on(summaries, drawing_id, &tag),
                    tag,
                };
                self.pidlocate_go(project_dir, index, drawing_id, wanted)
            }
            [first, ..] => {
                self.command_line.push_error_coded(
                    LOCATE_AMBIGUOUS,
                    &format!(
                        "PIDLOCATE: {LOCATE_AMBIGUOUS}: {tag} 在 {} 张图上都有；写上图号再定位，比如 PIDLOCATE {tag} {first}。",
                        drawings.len()
                    ),
                );
                for place in &places {
                    self.command_line.push_output(&format!(
                        "  {} {}",
                        place.drawing_id,
                        point(place.at)
                    ));
                }
                Task::none()
            }
        }
    }

    /// 哪张图上都没有：说是项目里没有这个位号、没有这张图、那张图读不出来，还是那张图上
    /// 没有。
    fn pidlocate_not_found(&mut self, index: &ProjectIndex, tag: &str, drawing: Option<&str>) {
        let why = match drawing {
            None => format!("项目里哪张图上都没有 {tag}"),
            Some(id) => match index
                .drawings
                .iter()
                .find(|indexed| indexed.drawing_id.eq_ignore_ascii_case(id))
            {
                None => format!("项目里没有图号是 {id} 的图"),
                Some(indexed) => match &indexed.error {
                    Some(error) => format!("{} 读不出来：{error}", indexed.drawing_id),
                    None => format!("{} 上没有 {tag}", indexed.drawing_id),
                },
            },
        };
        self.command_line.push_error_coded(
            LOCATE_NOT_FOUND,
            &format!("PIDLOCATE: {LOCATE_NOT_FOUND}: {why}。"),
        );
        if drawing.is_none() {
            self.pidindex_incomplete(index);
        }
    }

    /// 去 `drawing_id` 那张图上找 `wanted`：有标签页开着它就切过去当场找；没有就打开它，
    /// 打开以后接着找（`on_pid_locate_opened`）。
    fn pidlocate_go(
        &mut self,
        project_dir: &Path,
        index: &ProjectIndex,
        drawing_id: &str,
        wanted: Wanted,
    ) -> Task<Message> {
        let Some(file) = index
            .drawings
            .iter()
            .find(|indexed| indexed.drawing_id == drawing_id)
            .and_then(|indexed| indexed.file.as_deref())
        else {
            self.command_line.push_error_coded(
                LOCATE_NOT_FOUND,
                &format!("PIDLOCATE: {LOCATE_NOT_FOUND}: {drawing_id} 没有文件，打不开。"),
            );
            return Task::none();
        };
        let path = project_dir.join(file);
        if let Some(tab) = self.pidlocate_tab(&path) {
            let switched = if tab == self.active_tab {
                Task::none()
            } else {
                self.update(Message::TabSwitch(tab))
            };
            if !self.pidlocate_in(tab, &wanted, &format!("{drawing_id} 上")) {
                self.pidlocate_gone(drawing_id, &wanted.tag);
            }
            return switched;
        }
        self.command_line.push_info(&format!(
            "PIDLOCATE: {} 在 {drawing_id} 上，打开它。",
            wanted.tag
        ));
        let task = self.update(Message::OpenRecent(path.clone()));
        // 打开可能没起来（文件刚被删、一次只开一张图而当前图没存），那时没有这次打开。
        let open_id = self
            .opening
            .as_ref()
            .filter(|open| {
                open.source_path
                    .as_deref()
                    .is_some_and(|source| same_file(source) == same_file(&path))
            })
            .map(|open| open.id);
        match open_id {
            Some(open_id) => {
                self.pid_index.opening = Some(Opening {
                    open_id,
                    drawing_id: drawing_id.to_string(),
                    wanted,
                });
            }
            None => self.pidlocate_not_opened(drawing_id, &wanted.tag),
        }
        task
    }

    /// 编号是 `open_id` 的那次打开做完了，打开的是 `path`：是为定位打开的，就在它的
    /// 标签页上接着找。
    pub(in crate::app) fn on_pid_locate_opened(&mut self, open_id: u64, path: &Path) {
        let Some(opening) = self
            .pid_index
            .opening
            .take_if(|opening| opening.open_id == open_id)
        else {
            return;
        };
        let tab = self.pidlocate_tab(path).unwrap_or(self.active_tab);
        let place = format!("{} 上", opening.drawing_id);
        if !self.pidlocate_in(tab, &opening.wanted, &place) {
            self.pidlocate_gone(&opening.drawing_id, &opening.wanted.tag);
        }
    }

    /// 编号是 `open_id` 的那次打开失败了：是为定位打开的，就不再等它，说没定位。
    pub(in crate::app) fn on_pid_locate_open_failed(&mut self, open_id: u64) {
        if let Some(opening) = self
            .pid_index
            .opening
            .take_if(|opening| opening.open_id == open_id)
        {
            self.pidlocate_not_opened(&opening.drawing_id, &opening.wanted.tag);
        }
    }

    /// 要去的图没打开成。
    fn pidlocate_not_opened(&mut self, drawing_id: &str, tag: &str) {
        self.command_line
            .push_error(&format!("PIDLOCATE: {drawing_id} 没打开，没定位到 {tag}。"));
    }

    /// 开着 `path` 的标签页。先按图纸记的文件认，与扫描认打开着的图用的是同一个；再按
    /// 标签页打开的文件认。
    fn pidlocate_tab(&self, path: &Path) -> Option<usize> {
        let wanted = same_file(path);
        self.tabs
            .iter()
            .position(|tab| {
                tab.scene
                    .document
                    .source_path
                    .as_deref()
                    .is_some_and(|source| same_file(Path::new(source)) == wanted)
            })
            .or_else(|| self.tab_showing(path))
    }

    /// 扫的时候在，到图上现找却没有了：图在扫过以后改了。
    fn pidlocate_gone(&mut self, drawing_id: &str, tag: &str) {
        self.command_line.push_error_coded(
            LOCATE_NOT_FOUND,
            &format!(
                "PIDLOCATE: {LOCATE_NOT_FOUND}: {drawing_id} 上现在找不到 {tag}，图在扫过以后改了；再定位一次。"
            ),
        );
    }

    /// 在第 `i` 个标签页的图上按 `wanted` 现找（§5.2：照图现在的样子，不用摘要里记的
    /// 句柄）。找到了就只选中它们、缩放到它们，说在 `place` 的哪里；画不出样子的按外框
    /// 和定位点缩放。找不到什么也不动，返回 `false`。
    fn pidlocate_in(&mut self, i: usize, wanted: &Wanted, place: &str) -> bool {
        let meta = SummaryMeta {
            drawing_id: String::new(),
            file: None,
            pbs: None,
            source: SummarySource::Open,
            fingerprint: None,
        };
        let summary = summarize(
            &self.tabs[i].scene.document,
            &self.pid_symbols,
            meta,
            DEFAULT_TOLERANCE,
        );
        let found: Vec<&SummaryObject> = summary
            .objects
            .iter()
            .filter(|object| {
                object.own_tag
                    && object.tag.eq_ignore_ascii_case(&wanted.tag)
                    && wanted
                        .type_code
                        .as_deref()
                        .is_none_or(|code| object.type_code == code)
            })
            .collect();
        let handles: Vec<document::Handle> = found
            .iter()
            .filter_map(|object| parse_handle(&object.handle))
            .collect();
        if handles.is_empty() {
            return false;
        }
        let scene = &mut self.tabs[i].scene;
        scene.deselect_all();
        scene.select_entities(&handles);
        scene.remember_current_view();
        if !scene.zoom_to_entities(&handles) {
            let (min, max) = frame(&found);
            scene.zoom_to_window(min, max);
        }
        self.refresh_properties();
        let what: Vec<String> = found
            .iter()
            .map(|object| format!("{} {} {}", object.type_label, object.tag, point(object.at)))
            .collect();
        self.command_line.push_output(&format!(
            "PIDLOCATE: {place}的 {}，选中了。",
            what.join("、")
        ));
        true
    }
}

/// `drawing_id` 那张图上位号是 `tag` 的对象的类型编码：只有一种就按它找，几种都有就不论
/// 类型。
fn type_code_on(summaries: &[DrawingSummary], drawing_id: &str, tag: &str) -> Option<String> {
    let codes: BTreeSet<&str> = summaries
        .iter()
        .filter(|summary| summary.drawing_id == drawing_id)
        .flat_map(|summary| &summary.objects)
        .filter(|object| object.own_tag && object.tag.eq_ignore_ascii_case(tag))
        .map(|object| object.type_code.as_str())
        .collect();
    if codes.len() == 1 {
        codes.first().map(|code| (*code).to_string())
    } else {
        None
    }
}

/// 画不出样子时缩放到的范围（§5.2 按坐标缩放）：对象外框的并集，没有外框的取定位点；
/// 四边留出长边的一成，至少 [`FRAME_MARGIN`]。
fn frame(objects: &[&SummaryObject]) -> (glam::Vec3, glam::Vec3) {
    let mut low = glam::DVec2::splat(f64::INFINITY);
    let mut high = glam::DVec2::splat(f64::NEG_INFINITY);
    for object in objects {
        let [x0, y0, x1, y1] = object.bbox;
        let (from, to) = if x1 > x0 || y1 > y0 {
            (glam::DVec2::new(x0, y0), glam::DVec2::new(x1, y1))
        } else {
            let at = glam::DVec2::from(object.at);
            (at, at)
        };
        low = low.min(from);
        high = high.max(to);
    }
    let margin = ((high - low).max_element() / 10.0).max(FRAME_MARGIN);
    (
        (low - margin).extend(0.0).as_vec3(),
        (high + margin).extend(0.0).as_vec3(),
    )
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
    use crate::pid::topology::{handle_text, TopologyTable};
    use iced::futures::StreamExt;
    use serde_json::{json, Value};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

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

    /// 像在命令行上敲进去那样跑一行，它在后台起的任务也一直跑到完。
    fn run(app: &mut OpenCADStudio, line: &str) -> Ran {
        app.command_line.history.clear();
        app.command_line.last_error = None;
        app.command_line.last_error_code = None;
        let task = app.run_command_line(line);
        app.drive_headless_task(task).expect("命令的任务");
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
            "PIDINDEX CANCEL NOW",
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

    // ── 后台扫描 ──

    fn name(path: &Path) -> String {
        path.file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    }

    /// 命令行上到现在说过的。
    fn said(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 在临时目录里存一张 `objects` 个对象的合成图。
    fn write_synthetic(scratch: &Scratch, file: &str, objects: usize) {
        let bytes = crate::io::save_to_bytes(
            &synthetic_drawing(objects),
            "dwg",
            document::DxfVersion::AC1032,
        )
        .expect("存 DWG");
        fs::write(scratch.path(file), bytes).expect("写文件");
    }

    /// 记下读了哪些文件的读法。
    fn counting() -> (Loader, Arc<Mutex<Vec<String>>>) {
        let read = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&read);
        let loader: Loader = Arc::new(move |path: &Path| {
            log.lock().expect("读文件的记录").push(name(path));
            crate::io::load_file(path)
        });
        (loader, read)
    }

    /// 卡着等放行的读法：每读一个文件先记下名字，再等——`permit` 放过一个，`open`
    /// 全部放过。
    #[derive(Clone, Default)]
    struct Gate(Arc<(Mutex<Passes>, Condvar)>);

    #[derive(Default)]
    struct Passes {
        entered: Vec<String>,
        permits: usize,
        open: bool,
    }

    impl Gate {
        fn loader(&self) -> Loader {
            let gate = self.clone();
            Arc::new(move |path: &Path| {
                gate.enter(path);
                crate::io::load_file(path)
            })
        }

        fn enter(&self, path: &Path) {
            let (passes, changed) = &*self.0;
            let mut passes = passes.lock().expect("闸");
            passes.entered.push(name(path));
            changed.notify_all();
            while !passes.open && passes.permits == 0 {
                passes = changed.wait(passes).expect("闸");
            }
            if !passes.open {
                passes.permits -= 1;
            }
        }

        /// 等后台读到第 `count` 个文件（进了读法，可能还卡着），返回读到的那些。
        fn wait_entered(&self, count: usize) -> Vec<String> {
            let (passes, changed) = &*self.0;
            let passes = passes.lock().expect("闸");
            let (passes, waited) = changed
                .wait_timeout_while(passes, Duration::from_secs(10), |passes| {
                    passes.entered.len() < count
                })
                .expect("闸");
            assert!(
                !waited.timed_out(),
                "等了 10 秒，后台只读到 {:?}",
                passes.entered
            );
            passes.entered.clone()
        }

        fn permit(&self) {
            let (passes, changed) = &*self.0;
            passes.lock().expect("闸").permits += 1;
            changed.notify_all();
        }

        fn open(&self) {
            let (passes, changed) = &*self.0;
            passes.lock().expect("闸").open = true;
            changed.notify_all();
        }

        fn entered(&self) -> Vec<String> {
            let (passes, _) = &*self.0;
            passes.lock().expect("闸").entered.clone()
        }
    }

    /// 后台任务交出的下一条，像界面线程收到它那样交给程序；任务完了返回 `false`。
    fn deliver(
        app: &mut OpenCADStudio,
        stream: &mut (impl iced::futures::Stream<Item = iced_runtime::Action<Message>> + Unpin),
    ) -> bool {
        match iced::futures::executor::block_on(stream.next()) {
            Some(iced_runtime::Action::Output(message)) => {
                let next = app.update(message);
                app.drive_headless_task(next).expect("消息的任务");
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// 扫描期间程序照常应答别的请求（§4）：读文件的办法卡着不放，`PIDINDEX` 先回来，
    /// 后台还在读；这时画一根管线、再要一次 `DUPLICATES` 都有应答。放行以后，总数和重号
    /// 按下命令的先后接着说。
    #[test]
    fn pidindex_answers_other_requests_while_it_reads() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let gate = Gate::default();
        app.pid_index.loader = gate.loader();
        app.command_line.history.clear();

        let task = app.run_command_line("PIDINDEX");
        assert_eq!(gate.wait_entered(1), ["PID-1102.dwg"]);
        let progress = app
            .pid_index
            .scan
            .as_ref()
            .map(|scan| (scan.done, scan.total));
        assert_eq!(progress, Some((0, 1)));
        assert!(said(&app).contains("要从磁盘读 1 个文件"), "{}", said(&app));
        assert!(
            !said(&app).contains("项目目录"),
            "还没读完就说了总数：{}",
            said(&app)
        );

        draw(&mut app, "PIDPIPE", &[[0.0, -200.0], [100.0, -200.0]]);
        let asked = request(&mut app, json!({"op": "run", "cmd": "PIDINDEX DUPLICATES"}));
        assert_eq!(asked["ok"], true, "{asked}");
        assert!(said(&app).contains("还在读"), "{}", said(&app));

        gate.open();
        app.drive_headless_task(task).expect("后台扫描的任务");
        let text = said(&app);
        assert!(
            text.contains("2 张图，跨图重号 1 处，连接符配成 1 对、0 个没配上"),
            "{text}"
        );
        assert!(
            text.contains("PIDINDEX DUPLICATES: 跨图重号 1 处。"),
            "{text}"
        );
        assert!(app.pid_index.scan.is_none());
    }

    /// 取消（§4）：后台读到第二个文件时 `PIDINDEX CANCEL`，第三个不再读；读好的两个记在
    /// 缓存里，再扫一次只读第三个。
    #[test]
    fn pidindex_cancel_stops_reading_and_keeps_what_it_read() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        write_synthetic(&scratch, "PID-1103.dwg", 20);
        write_synthetic(&scratch, "PID-1104.dwg", 20);
        let gate = Gate::default();
        app.pid_index.loader = gate.loader();
        app.command_line.history.clear();

        let task = app.run_command_line("PIDINDEX");
        let mut stream = iced_runtime::task::into_stream(task).expect("后台扫描的任务");
        assert_eq!(gate.wait_entered(1), ["PID-1102.dwg"]);
        gate.permit();
        while !said(&app).contains("读了 1/3 个文件") {
            assert!(
                deliver(&mut app, &mut stream),
                "任务完了还没报进度：{}",
                said(&app)
            );
        }
        assert_eq!(gate.wait_entered(2), ["PID-1102.dwg", "PID-1103.dwg"]);

        let ran = run(&mut app, "PIDINDEX CANCEL");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(ran.said.contains("取消了"), "{}", ran.said);
        gate.permit();
        while deliver(&mut app, &mut stream) {}
        assert_eq!(
            gate.entered(),
            ["PID-1102.dwg", "PID-1103.dwg"],
            "取消以后不再读下一个"
        );
        assert_eq!(app.pid_index.cache.entries.len(), 2, "读好的两个都记着");
        assert!(
            !said(&app).contains("项目目录"),
            "取消了就不再说总数：{}",
            said(&app)
        );

        let (loader, read) = counting();
        app.pid_index.loader = loader;
        let ran = run(&mut app, "PIDINDEX");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert!(ran.said.contains("4 张图"), "{}", ran.said);
        assert_eq!(*read.lock().expect("读文件的记录"), ["PID-1104.dwg"]);
    }

    const RESCANS: usize = 3;
    const RESCAN_BUDGET: Duration = Duration::from_secs(1);

    /// 100 张图的项目，文件都没变时重扫不超过 1 秒（性能预算）。默认不跑：要先存 100
    /// 张图、把它们读一遍。
    #[test]
    #[ignore]
    fn pidindex_rescan_100() {
        let scratch = Scratch::new();
        let bytes =
            crate::io::save_to_bytes(&synthetic_drawing(200), "dwg", document::DxfVersion::AC1032)
                .expect("存 DWG");
        for number in 2001..2101 {
            fs::write(scratch.path(&format!("PID-{number}.dwg")), &bytes).expect("写文件");
        }
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document = synthetic_drawing(200);
        app.tabs[i].scene.document.source_path =
            Some(scratch.path("PID-2001.dwg").to_string_lossy().into_owned());
        let (loader, read) = counting();
        app.pid_index.loader = loader;

        let ran = run(&mut app, "PIDINDEX");
        assert!(ran.said.contains("100 张图"), "{}", ran.said);
        assert_eq!(
            read.lock().expect("读文件的记录").len(),
            99,
            "打开着的那张不读"
        );

        let mut samples = Vec::with_capacity(RESCANS);
        for rescan in 1..=RESCANS {
            let started = Instant::now();
            let ran = run(&mut app, "PIDINDEX");
            let elapsed = started.elapsed();
            println!(
                "IX-6 rescan {rescan}/{RESCANS}: {:.2} ms",
                elapsed.as_secs_f64() * 1e3
            );
            assert!(
                ran.said.contains("100 张图"),
                "rescan {rescan}: {}",
                ran.said
            );
            samples.push(elapsed);
        }
        assert_eq!(
            read.lock().expect("读文件的记录").len(),
            99,
            "文件没变，重扫一个也不读"
        );
        samples.sort();
        let median = samples[RESCANS / 2];
        let shown: Vec<String> = samples
            .iter()
            .map(|sample| format!("{:.2}", sample.as_secs_f64() * 1e3))
            .collect();
        println!(
            "IX-6 median {:.2} ms of {shown:?}, budget {} ms",
            median.as_secs_f64() * 1e3,
            RESCAN_BUDGET.as_millis()
        );
        assert!(
            median <= RESCAN_BUDGET,
            "IX-6 超预算：中位数 {:.2} ms（{shown:?}），预算 {} ms",
            median.as_secs_f64() * 1e3,
            RESCAN_BUDGET.as_millis()
        );
    }

    // ── PIDLOCATE ──

    /// 第 `i` 个标签页上位号是 `tag` 的那个对象，照图现在的样子做摘要找。
    fn object_on(app: &OpenCADStudio, i: usize, tag: &str) -> SummaryObject {
        let meta = SummaryMeta {
            drawing_id: String::new(),
            file: None,
            pbs: None,
            source: SummarySource::Open,
            fingerprint: None,
        };
        let summary = summarize(
            &app.tabs[i].scene.document,
            &app.pid_symbols,
            meta,
            DEFAULT_TOLERANCE,
        );
        let mut found: Vec<SummaryObject> = summary
            .objects
            .into_iter()
            .filter(|object| object.own_tag && object.tag == tag)
            .collect();
        assert_eq!(found.len(), 1, "{tag}：{found:#?}");
        found.remove(0)
    }

    /// 第 `i` 个标签页上选中的，按句柄排。
    fn selected(app: &OpenCADStudio, i: usize) -> Vec<String> {
        let mut handles: Vec<String> = app.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle_text(handle))
            .collect();
        handles.sort();
        handles
    }

    /// 把第 `i` 个标签页的视图挪到远处，看定位是不是把它拉回来。
    fn look_away(app: &OpenCADStudio, i: usize) {
        app.tabs[i].scene.camera.borrow_mut().target = glam::DVec3::new(5000.0, 5000.0, 0.0);
    }

    /// 视图的中心离 `object` 外框的中心不超过外框的长边：缩放到的是它，不是旁边的
    /// 东西。缩放的范围连着位号的字，中心会偏一点。
    fn looks_at(app: &OpenCADStudio, i: usize, object: &SummaryObject) -> bool {
        let target = app.tabs[i].scene.camera.borrow().target;
        let [x0, y0, x1, y1] = object.bbox;
        let center = glam::DVec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        target.truncate().distance(center) <= (x1 - x0).max(y1 - y0)
    }

    /// 位号就在当前图上：选中的就是它，视图缩放到它。不扫项目，一个文件也不读。
    #[test]
    fn pidlocate_selects_the_tag_on_the_current_drawing_and_zooms_to_it() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let (loader, read) = counting();
        app.pid_index.loader = loader;
        let i = app.active_tab;
        let pump = object_on(&app, i, "P-1001");
        look_away(&app, i);

        let ran = run(&mut app, "PIDLOCATE P-1001");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(app.active_tab, i);
        assert_eq!(selected(&app, i), [pump.handle.as_str()], "{}", ran.said);
        assert!(
            looks_at(&app, i, &pump),
            "{:?}",
            app.tabs[i].scene.camera.borrow().target
        );
        assert!(
            read.lock().expect("读文件的记录").is_empty(),
            "{}",
            ran.said
        );
    }

    /// 位号只在一张没打开的图上：打开它、切过去、选中，视图缩放到它。再定位一次不再
    /// 打开第二份，切到开着的那一页。
    #[test]
    fn pidlocate_opens_the_drawing_the_tag_is_on_and_selects_it() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let first = app.active_tab;
        let tabs = app.tabs.len();

        let ran = run(&mut app, "PIDLOCATE P-1002");
        assert_eq!(ran.error, None, "{}", ran.said);
        let i = app.active_tab;
        assert_eq!(app.tabs.len(), tabs + 1, "{}", ran.said);
        assert_eq!(
            app.tab_showing(&scratch.path("PID-1102.dwg")),
            Some(i),
            "{}",
            ran.said
        );
        let pump = object_on(&app, i, "P-1002");
        assert_eq!(selected(&app, i), [pump.handle.as_str()], "{}", ran.said);
        assert!(
            looks_at(&app, i, &pump),
            "{:?}",
            app.tabs[i].scene.camera.borrow().target
        );

        let _ = app.update(Message::TabSwitch(first));
        look_away(&app, i);
        let ran = run(&mut app, "PIDLOCATE P-1002");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(app.tabs.len(), tabs + 1, "打开了第二份：{}", ran.said);
        assert_eq!(app.active_tab, i, "{}", ran.said);
        assert!(
            looks_at(&app, i, &pump),
            "{:?}",
            app.tabs[i].scene.camera.borrow().target
        );
    }

    /// 哪张图上都没有：`LOCATE_NOT_FOUND`。有多处又没给图号：`LOCATE_AMBIGUOUS`，逐处
    /// 列出来，不打开图。给了图号就去那一张；项目里没有这个图号、那张图上没有这个位号，
    /// 也是 `LOCATE_NOT_FOUND`。
    #[test]
    fn pidlocate_says_when_a_tag_is_nowhere_or_in_several_places() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        fs::copy(scratch.path("PID-1102.dwg"), scratch.path("PID-1103.dwg")).expect("复制图");
        let tabs = app.tabs.len();

        let ran = run(&mut app, "PIDLOCATE P-9999");
        assert_eq!(ran.code.as_deref(), Some(LOCATE_NOT_FOUND), "{}", ran.said);

        let ran = run(&mut app, "PIDLOCATE P-1002");
        assert_eq!(ran.code.as_deref(), Some(LOCATE_AMBIGUOUS), "{}", ran.said);
        for place in ["PID-1102 (0, -60)", "PID-1103 (0, -60)"] {
            assert!(ran.said.contains(place), "{place}：{}", ran.said);
        }
        assert_eq!(app.tabs.len(), tabs, "{}", ran.said);

        for line in ["PIDLOCATE P-1002 PID-9999", "PIDLOCATE P-1002 PID-1101"] {
            let ran = run(&mut app, line);
            assert_eq!(
                ran.code.as_deref(),
                Some(LOCATE_NOT_FOUND),
                "{line}：{}",
                ran.said
            );
        }

        let ran = run(&mut app, "PIDLOCATE P-1002 PID-1103");
        assert_eq!(ran.error, None, "{}", ran.said);
        let i = app.active_tab;
        assert_eq!(
            app.tab_showing(&scratch.path("PID-1103.dwg")),
            Some(i),
            "{}",
            ran.said
        );
        let pump = object_on(&app, i, "P-1002");
        assert_eq!(selected(&app, i), [pump.handle.as_str()], "{}", ran.said);
    }

    /// 要去的图没打开成（这里是一次只开一张图、当前图又没存）：当场说没定位到，不再等
    /// 它；以后照常打开这张图，不会接着去选。
    #[test]
    fn pidlocate_does_not_wait_for_a_drawing_it_could_not_open() {
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        let first = app.active_tab;
        app.sdi = true;
        app.tabs[first].dirty = true;

        let ran = run(&mut app, "PIDLOCATE P-1002");
        assert!(
            ran.error
                .is_some_and(|error| error.starts_with("PIDLOCATE") && error.contains("没打开")),
            "{}",
            ran.said
        );

        app.sdi = false;
        let task = app.update(Message::OpenRecent(scratch.path("PID-1102.dwg")));
        app.drive_headless_task(task).expect("打开图");
        let i = app.active_tab;
        assert_eq!(app.tab_showing(&scratch.path("PID-1102.dwg")), Some(i));
        let picked = selected(&app, i);
        assert!(picked.is_empty(), "{picked:?}");
    }

    /// 登记了；只读会话里能用，位号不分大小写，不留撤销步。缺位号、多写了东西不带错误码
    /// 地说怎么写；当前图上没有、当前图又没存过，说没有项目目录。
    #[test]
    fn pidlocate_is_registered_reads_only_and_says_how_it_is_written() {
        assert!(crate::command::all_registered_command_names().contains(&"PIDLOCATE"));
        let scratch = Scratch::new();
        let mut app = project(&scratch);
        app.collab.readonly = true;
        let steps = undo_steps(&app);
        let i = app.active_tab;
        let pump = object_on(&app, i, "P-1001");
        assert!(!app.readonly_refuses_command("PIDLOCATE P-1001"));
        let ran = run(&mut app, "pidlocate p-1001");
        assert_eq!(ran.error, None, "{}", ran.said);
        assert_eq!(selected(&app, i), [pump.handle.as_str()], "{}", ran.said);
        assert_eq!(undo_steps(&app), steps);

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for line in ["PIDLOCATE", "PIDLOCATE P-1001 PID-1101 NOW"] {
            let ran = run(&mut app, line);
            assert_eq!(ran.code, None, "{line}: {}", ran.said);
            assert!(
                ran.error
                    .is_some_and(|error| error.starts_with("PIDLOCATE")),
                "{line}: {}",
                ran.said
            );
        }
        let ran = run(&mut app, "PIDLOCATE P-1001");
        assert_eq!(
            ran.code.as_deref(),
            Some(INDEX_NO_PROJECT_DIR),
            "{}",
            ran.said
        );
        assert!(
            ran.error
                .is_some_and(|error| error.starts_with("PIDLOCATE")),
            "{}",
            ran.said
        );
    }
}
