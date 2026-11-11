//! The restore cache, in the details of the backup itself.
//!
//! A backup that has to be unpacked is unpacked into a folder of its own in
//! the restore cache, and the folder is kept for the next time
//! ([`crate::io::pid_backup`]). The backup's details say where the cache is,
//! what this backup's restore and the other restores there weigh, and carry
//! the buttons that clear the others and move the cache to another folder:
//!
//! ```text
//! Restore cache
//! Cache folder            C:\Users\me\AppData\Local\Temp\ocs-pid-backups (the default folder)
//! This backup's restore   160.2 MB in 813 file(s)
//! Other restores          2, 202.0 MB together
//! [Clear the other restores]
//! [Change the cache folder…]
//! ```
//!
//! Measuring walks the folders and clearing removes them, so both happen
//! off the UI thread: the cache is measured when the backup's row is
//! selected, and again after a clearing or a move. The folder is a
//! preference: the application keeps it in its settings, and it outlives
//! the backup that is open.
//!
//! Each measuring and clearing is counted, and its answer carries the count
//! it began under. Only the latest answer is shown, so one that was asked
//! about another folder, or for another backup, never shows as this one's.

use std::path::{Path, PathBuf};

use super::{human_size, DetailLine, Effect, NodeId, PidBackupBrowser, PidBackupMsg};
use crate::io::pid_backup::cache::CacheUsage;
use crate::io::pid_backup::default_cache_root;
use crate::io::pid_backup_browse::BackupIndex;
use crate::{t, tf};

/// What the panel knows of the restore cache.
#[derive(Debug, Clone, Default)]
pub struct CacheState {
    /// The folder set for it; `None`: the default one.
    folder: Option<PathBuf>,
    /// What it held when it was last measured or cleared, or why that
    /// failed.
    usage: Option<Result<CacheUsage, String>>,
    /// Counts the measurings and clearings asked for.
    asked: u64,
    /// The latest of them has not been answered.
    busy: bool,
    /// A clearing is under way.
    clearing: bool,
    /// The command line asked what the cache holds: it is told when the
    /// measurement arrives.
    report: bool,
    /// What the command line has to say about the cache, until it has.
    outcome: Option<Result<String, String>>,
}

impl CacheState {
    /// The folder restores go into now.
    fn root(&self) -> PathBuf {
        self.folder.clone().unwrap_or_else(default_cache_root)
    }

    /// The folder as the panel and the command line name it.
    fn folder_text(&self) -> String {
        let root = self.root();
        if self.folder.is_none() {
            tf!("{} (the default folder)", root.display()).into_owned()
        } else {
            root.display().to_string()
        }
    }

    /// The backup went away, or the cache moved: what was measured was
    /// measured for what was there, and so is what is on its way -- counting
    /// past it leaves its answer unshown.
    pub(super) fn forget(&mut self) {
        self.usage = None;
        if self.busy {
            self.asked += 1;
            self.busy = false;
        }
    }
}

/// What the user did about the restore cache, and what came back.
#[derive(Debug, Clone)]
pub enum CacheMsg {
    /// The cache was measured or cleared: what it holds now, or why not.
    /// `asked` is the count the work began under.
    Measured {
        asked: u64,
        result: Result<CacheUsage, String>,
    },
    /// The button that clears the other restores, and `PIDBACKUPOPEN
    /// Cache=Clear`.
    Clear,
    /// The button that changes the cache folder: ask for one.
    PickFolder,
    /// The folder dialog came back; `None` when the user cancelled.
    FolderPicked(Option<PathBuf>),
    /// Keep restores in this folder from now on; `None`: the default one.
    Folder(Option<PathBuf>),
    /// `PIDBACKUPOPEN Cache`: say on the command line what the cache holds.
    Report,
}

/// The folder set for the restore cache; `None` while it is the default.
pub fn cache_folder(state: &PidBackupBrowser) -> Option<&Path> {
    state.cache.folder.as_deref()
}

/// Set the cache folder as the settings state it: nothing is measured or
/// moved, the next restore goes there.
pub fn set_cache_folder(state: &mut PidBackupBrowser, folder: Option<PathBuf>) {
    state.cache.folder = normal(folder);
    state.cache.forget();
}

/// What the cache was last seen to hold.
pub fn cache_usage(state: &PidBackupBrowser) -> Option<&CacheUsage> {
    match &state.cache.usage {
        Some(Ok(usage)) => Some(usage),
        _ => None,
    }
}

/// What the command line has to say about the cache since this was last
/// asked: what a clearing freed or why it failed, where restores are kept
/// after a move, what the cache holds when that was asked.
pub fn take_cache_outcome(state: &mut PidBackupBrowser) -> Option<Result<String, String>> {
    state.cache.outcome.take()
}

/// A folder as the setting keeps it: nothing for an empty path and for the
/// default folder itself, so that the default stays the default when the
/// temp folder moves.
fn normal(folder: Option<PathBuf>) -> Option<PathBuf> {
    folder
        .filter(|folder| !folder.as_os_str().is_empty())
        .filter(|folder| *folder != default_cache_root())
}

/// The restore folder of the open backup, as the cache would hold it.
fn current(state: &PidBackupBrowser) -> Option<PathBuf> {
    state.index().map(|index| index.restored.clone())
}

/// Count one more measuring or clearing, and give its count.
fn ask(state: &mut PidBackupBrowser) -> u64 {
    state.cache.asked += 1;
    state.cache.busy = true;
    state.cache.asked
}

/// Measure the cache, unless something is under way already: a measuring,
/// or a clearing, which ends with a measurement of its own.
pub(super) fn measure(state: &mut PidBackupBrowser) -> Effect {
    if state.cache.busy || state.cache.clearing {
        return Effect::None;
    }
    Effect::MeasureCache {
        asked: ask(state),
        root: state.cache.root(),
        current: current(state),
    }
}

/// Whether the details on screen are the ones the cache's lines are in.
fn on_screen(state: &PidBackupBrowser) -> bool {
    state.index().is_some() && state.selected.as_ref().is_some_and(shows)
}

/// What the command line is told the cache holds.
fn summary(cache: &CacheState, usage: &CacheUsage) -> String {
    match &usage.current {
        Some(restore) => tf!(
            "the restore cache {} holds this backup's restore of {} and {} other restore(s) of {} together",
            cache.folder_text(),
            human_size(restore.bytes),
            usage.others,
            human_size(usage.other_bytes)
        ),
        None => tf!(
            "the restore cache {} holds {} restore(s) of {} together",
            cache.folder_text(),
            usage.others,
            human_size(usage.other_bytes)
        ),
    }
    .into_owned()
}

/// "2 restore(s) removed, 202.0 MB freed".
fn freed_text(removed: usize, bytes: u64) -> String {
    tf!("{} restore(s) removed, {} freed", removed, human_size(bytes)).into_owned()
}

/// Apply `msg` and say what the application should do next.
pub(super) fn update(state: &mut PidBackupBrowser, msg: CacheMsg) -> Effect {
    match msg {
        CacheMsg::Measured { asked, result } => {
            // Only a clearing says what it freed, and only a clearing
            // fails. What it did is news wherever the cache is by now.
            let freed = match &result {
                Ok(usage) => usage.freed.map(|(removed, bytes)| Ok(freed_text(removed, bytes))),
                Err(error) => Some(Err(error.clone())),
            };
            let cleared = freed.is_some();
            if let Some(freed) = freed {
                state.cache.clearing = false;
                state.cache.outcome = Some(freed);
            }
            if asked != state.cache.asked {
                // Asked about another folder or beside another backup. What
                // is on screen now is measured for itself.
                return if on_screen(state) && state.cache.usage.is_none() {
                    measure(state)
                } else {
                    Effect::None
                };
            }
            state.cache.busy = false;
            if std::mem::take(&mut state.cache.report) && !cleared {
                if let Ok(usage) = &result {
                    state.cache.outcome = Some(Ok(summary(&state.cache, usage)));
                }
            }
            state.cache.usage = Some(result);
            Effect::None
        }
        CacheMsg::Clear => {
            if state.cache.clearing {
                state.cache.outcome = Some(Err(
                    t!("the restore cache is being cleared already").into_owned()
                ));
                return Effect::None;
            }
            state.cache.clearing = true;
            // A measurement on its way is of the cache before the clearing:
            // counting past it leaves it unshown.
            Effect::ClearCache {
                asked: ask(state),
                root: state.cache.root(),
                current: current(state),
            }
        }
        CacheMsg::PickFolder => Effect::PickCacheFolder,
        CacheMsg::FolderPicked(None) => Effect::None,
        CacheMsg::FolderPicked(Some(folder)) => update(state, CacheMsg::Folder(Some(folder))),
        CacheMsg::Folder(folder) => {
            let folder = normal(folder);
            if folder == state.cache.folder {
                state.cache.outcome = Some(Ok(tf!(
                    "restores are kept in {} already",
                    state.cache.folder_text()
                )
                .into_owned()));
                return Effect::None;
            }
            let before = state.cache.root();
            state.cache.folder = folder;
            state.cache.forget();
            state.cache.outcome = Some(Ok(tf!(
                "restores are kept in {} from now on; what {} holds stays there until it is cleared",
                state.cache.folder_text(),
                before.display()
            )
            .into_owned()));
            // The backup that is open stays where it was restored; what the
            // new folder holds is worth knowing where it is shown.
            if on_screen(state) {
                measure(state)
            } else {
                Effect::None
            }
        }
        CacheMsg::Report => {
            state.cache.report = true;
            // Nothing to start while something is under way: its answer is
            // the report.
            measure(state)
        }
    }
}

/// The cache's lines among the details of the backup itself.
pub(super) fn lines(state: &PidBackupBrowser, index: &BackupIndex, lines: &mut Vec<DetailLine>) {
    let cache = &state.cache;
    lines.push(DetailLine::Heading(t!("Restore cache").into_owned()));
    lines.push(DetailLine::Field(
        t!("Cache folder").into_owned(),
        cache.folder_text(),
    ));
    let mut others = 0;
    match &cache.usage {
        None => {}
        Some(Err(error)) => lines.push(DetailLine::Marker(error.clone())),
        Some(Ok(usage)) => {
            others = usage.others;
            lines.push(DetailLine::Field(
                t!("This backup's restore").into_owned(),
                match &usage.current {
                    Some(restore) => {
                        tf!("{} in {} file(s)", human_size(restore.bytes), restore.files)
                            .into_owned()
                    }
                    // A folder of loose drawings is read where it lies.
                    None if index.restored == index.source => {
                        t!("the folder is read where it lies; nothing is cached").into_owned()
                    }
                    None => tf!("not in this cache folder: {}", index.restored.display())
                        .into_owned(),
                },
            ));
            lines.push(DetailLine::Field(
                t!("Other restores").into_owned(),
                if usage.others == 0 {
                    t!("no other restores").into_owned()
                } else {
                    tf!("{}, {} together", usage.others, human_size(usage.other_bytes))
                        .into_owned()
                },
            ));
            if let Some((removed, bytes)) = usage.freed {
                lines.push(DetailLine::Marker(freed_text(removed, bytes)));
            }
        }
    }
    if cache.clearing {
        lines.push(DetailLine::Marker(t!("Clearing the cache…").into_owned()));
    } else if cache.busy {
        lines.push(DetailLine::Marker(t!("Measuring the cache…").into_owned()));
    }
    let action = |label: std::borrow::Cow<'static, str>, msg: CacheMsg| {
        DetailLine::Action(label.into_owned(), PidBackupMsg::Cache(msg))
    };
    if others > 0 && !cache.busy && !cache.clearing {
        lines.push(action(t!("Clear the other restores"), CacheMsg::Clear));
    }
    lines.push(action(t!("Change the cache folder…"), CacheMsg::PickFolder));
    if cache.folder.is_some() {
        lines.push(action(t!("Use the default folder"), CacheMsg::Folder(None)));
    }
}

/// Whether selecting `id` is a reason to measure the cache: its lines are
/// in the details of the backup itself.
pub(super) fn shows(id: &NodeId) -> bool {
    *id == NodeId::Root
}

/// [`crate::io::pid_backup::cache::usage`] on a worker thread.
pub async fn measure_in_background(root: PathBuf, current: Option<PathBuf>) -> CacheUsage {
    let unread = CacheUsage {
        root: root.clone(),
        ..CacheUsage::default()
    };
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("ocs-pid-backup-cache".into())
        .spawn(move || {
            let _ = sender.send(crate::io::pid_backup::cache::usage(
                &root,
                current.as_deref(),
            ));
        });
    match spawned {
        Ok(_) => receiver.await.unwrap_or(unread),
        Err(_) => unread,
    }
}

/// [`crate::io::pid_backup::cache::clear`] on a worker thread.
pub async fn clear_in_background(
    root: PathBuf,
    current: Option<PathBuf>,
) -> Result<CacheUsage, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("ocs-pid-backup-cache".into())
        .spawn(move || {
            let _ = sender.send(crate::io::pid_backup::cache::clear(
                &root,
                current.as_deref(),
            ));
        })
        .map_err(|error| tf!("cannot start clearing the cache: {}", error).into_owned())?;
    receiver
        .await
        .unwrap_or_else(|_| Err(t!("clearing the cache stopped before it finished").into_owned()))
}

#[cfg(test)]
mod tests {
    use super::super::tests::ready;
    use super::*;
    use crate::app::Message;
    use crate::io::pid_backup::cache::CachedRestore;
    use crate::ui::dock::Chrome;
    use crate::ui::window::pid_backup_browser::{detail_rows, update as panel, view};

    /// The backup's lines under the cache's heading, up to the next heading.
    fn shown(state: &PidBackupBrowser) -> Vec<(String, String)> {
        let rows = detail_rows(state, &NodeId::Root);
        let at = rows
            .iter()
            .position(|(key, _)| *key == t!("Restore cache"))
            .expect("the backup's details have the cache");
        let end = rows
            .iter()
            .position(|(key, _)| *key == t!("Check of the drawings"))
            .unwrap_or(rows.len());
        rows[at + 1..end].to_vec()
    }

    fn cache(msg: CacheMsg) -> PidBackupMsg {
        PidBackupMsg::Cache(msg)
    }

    /// The answer to the latest measuring or clearing.
    fn answer(state: &mut PidBackupBrowser, result: Result<CacheUsage, String>) -> Effect {
        let asked = state.cache.asked;
        panel(state, cache(CacheMsg::Measured { asked, result }))
    }

    /// The backup of the tests, its row selected and its cache measured.
    fn measured(usage: CacheUsage) -> PidBackupBrowser {
        let mut state = ready();
        assert!(matches!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::MeasureCache { .. }
        ));
        assert_eq!(answer(&mut state, Ok(usage)), Effect::None);
        state
    }

    /// What measuring the default folder finds for the test backup, whose
    /// restore is `D:/tmp/restore`.
    fn usage(others: usize, other_bytes: u64) -> CacheUsage {
        CacheUsage {
            root: default_cache_root(),
            current: Some(CachedRestore {
                folder: PathBuf::from("D:/tmp/restore"),
                bytes: 168_000_000,
                files: 813,
            }),
            others,
            other_bytes,
            freed: None,
        }
    }

    fn folder_line(default: bool, root: &Path) -> (String, String) {
        (
            t!("Cache folder").into_owned(),
            if default {
                tf!("{} (the default folder)", root.display()).into_owned()
            } else {
                root.display().to_string()
            },
        )
    }

    fn note(text: std::borrow::Cow<'static, str>) -> (String, String) {
        (text.into_owned(), String::new())
    }

    fn this_backup(value: std::borrow::Cow<'static, str>) -> (String, String) {
        (t!("This backup's restore").into_owned(), value.into_owned())
    }

    fn others(value: std::borrow::Cow<'static, str>) -> (String, String) {
        (t!("Other restores").into_owned(), value.into_owned())
    }

    #[test]
    fn selecting_the_backup_measures_the_cache_and_its_details_say_what_it_holds() {
        let mut state = ready();
        // Before anything is measured: where the cache is, and the way to
        // move it.
        assert_eq!(
            shown(&state),
            [
                folder_line(true, &default_cache_root()),
                note(t!("Change the cache folder…")),
            ]
        );
        // Selecting the backup measures it, off the UI thread.
        assert_eq!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::MeasureCache {
                asked: 1,
                root: default_cache_root(),
                current: Some(PathBuf::from("D:/tmp/restore")),
            }
        );
        assert!(shown(&state).contains(&note(t!("Measuring the cache…"))));
        // Selecting it again while that runs starts nothing more.
        assert_eq!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::None
        );
        // Another node is no reason to measure.
        assert_eq!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Files)),
            Effect::None
        );

        assert_eq!(answer(&mut state, Ok(usage(2, 211_812_352))), Effect::None);
        assert_eq!(
            shown(&state),
            [
                folder_line(true, &default_cache_root()),
                this_backup(tf!("{} in {} file(s)", "160.2 MB", 813)),
                others(tf!("{}, {} together", 2, "202.0 MB")),
                note(t!("Clear the other restores")),
                note(t!("Change the cache folder…")),
            ]
        );
        assert_eq!(cache_usage(&state).map(|usage| usage.others), Some(2));
        // Nothing of it is the command line's business.
        assert_eq!(take_cache_outcome(&mut state), None);
        // The next selection measures again: restores come and go.
        assert!(matches!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::MeasureCache { asked: 2, .. }
        ));
    }

    #[test]
    fn clearing_removes_the_other_restores_and_says_what_was_freed() {
        let mut state = measured(usage(2, 211_812_352));
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Clear)),
            Effect::ClearCache {
                asked: 2,
                root: default_cache_root(),
                current: Some(PathBuf::from("D:/tmp/restore")),
            }
        );
        // While it runs the panel says so, there is no button to press
        // again, and a second clearing from the command line is refused.
        let lines = shown(&state);
        assert!(lines.contains(&note(t!("Clearing the cache…"))));
        assert!(!lines.contains(&note(t!("Clear the other restores"))));
        assert_eq!(panel(&mut state, cache(CacheMsg::Clear)), Effect::None);
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Err(t!("the restore cache is being cleared already").into_owned()))
        );
        // Nor is the cache measured meanwhile: the clearing ends with a
        // measurement of its own.
        assert_eq!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::None
        );
        answer(
            &mut state,
            Ok(CacheUsage {
                freed: Some((2, 211_812_352)),
                ..usage(0, 0)
            }),
        );
        assert_eq!(
            shown(&state),
            [
                folder_line(true, &default_cache_root()),
                this_backup(tf!("{} in {} file(s)", "160.2 MB", 813)),
                others(t!("no other restores")),
                note(tf!("{} restore(s) removed, {} freed", 2, "202.0 MB")),
                note(t!("Change the cache folder…")),
            ]
        );
        // The command line is told as well, whoever asked for it.
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!("{} restore(s) removed, {} freed", 2, "202.0 MB").into_owned()))
        );
        assert_eq!(take_cache_outcome(&mut state), None);
        // A clearing that failed says why, in the panel and on the command
        // line, and the way to move the cache is still there.
        assert!(matches!(
            panel(&mut state, cache(CacheMsg::Clear)),
            Effect::ClearCache { asked: 3, .. }
        ));
        answer(&mut state, Err("cannot remove D:/x: in use".to_string()));
        assert_eq!(
            shown(&state),
            [
                folder_line(true, &default_cache_root()),
                note("cannot remove D:/x: in use".into()),
                note(t!("Change the cache folder…")),
            ]
        );
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Err("cannot remove D:/x: in use".to_string()))
        );
        // After a failure a clearing can be asked for again.
        assert!(matches!(
            panel(&mut state, cache(CacheMsg::Clear)),
            Effect::ClearCache { asked: 4, .. }
        ));
    }

    #[test]
    fn the_cache_folder_is_changed_from_a_dialog_or_a_message_and_set_back() {
        let mut state = measured(usage(0, 0));
        assert_eq!(cache_folder(&state), None);
        // The button asks for a folder; cancelling the dialog changes
        // nothing.
        assert_eq!(
            panel(&mut state, cache(CacheMsg::PickFolder)),
            Effect::PickCacheFolder
        );
        assert_eq!(
            panel(&mut state, cache(CacheMsg::FolderPicked(None))),
            Effect::None
        );
        assert_eq!(cache_folder(&state), None);
        assert_eq!(take_cache_outcome(&mut state), None);
        // A folder is kept, and measured at once: the open backup's restore
        // is not in it.
        let folder = PathBuf::from("E:/pid cache");
        assert_eq!(
            panel(&mut state, cache(CacheMsg::FolderPicked(Some(folder.clone())))),
            Effect::MeasureCache {
                asked: 2,
                root: folder.clone(),
                current: Some(PathBuf::from("D:/tmp/restore")),
            }
        );
        assert_eq!(cache_folder(&state), Some(folder.as_path()));
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!(
                "restores are kept in {} from now on; what {} holds stays there until it is cleared",
                folder.display(),
                default_cache_root().display()
            )
            .into_owned()))
        );
        answer(
            &mut state,
            Ok(CacheUsage {
                root: folder.clone(),
                ..CacheUsage::default()
            }),
        );
        assert_eq!(
            shown(&state),
            [
                folder_line(false, &folder),
                this_backup(tf!("not in this cache folder: {}", "D:/tmp/restore")),
                others(t!("no other restores")),
                note(t!("Change the cache folder…")),
                note(t!("Use the default folder")),
            ]
        );
        // The same folder again is no change, and says so.
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Folder(Some(folder.clone())))),
            Effect::None
        );
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!("restores are kept in {} already", folder.display()).into_owned()))
        );
        // The default folder named outright, and no folder, are the default.
        assert!(matches!(
            panel(&mut state, cache(CacheMsg::Folder(Some(default_cache_root())))),
            Effect::MeasureCache { root, .. } if root == default_cache_root()
        ));
        assert_eq!(cache_folder(&state), None);
        answer(&mut state, Ok(usage(0, 0)));
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Folder(Some(PathBuf::new())))),
            Effect::None
        );
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!(
                "restores are kept in {} already",
                tf!("{} (the default folder)", default_cache_root().display())
            )
            .into_owned()))
        );
        // The folder is the user's: a backup going away keeps it.
        panel(&mut state, cache(CacheMsg::Folder(Some(folder.clone()))));
        panel(&mut state, PidBackupMsg::Close);
        assert_eq!(cache_folder(&state), Some(folder.as_path()));
        // With no backup open a change measures nothing.
        assert_eq!(panel(&mut state, cache(CacheMsg::Folder(None))), Effect::None);
        // As the settings set it at startup: no effect at all.
        set_cache_folder(&mut state, Some(folder.clone()));
        assert_eq!(cache_folder(&state), Some(folder.as_path()));
        set_cache_folder(&mut state, Some(default_cache_root()));
        assert_eq!(cache_folder(&state), None);
    }

    #[test]
    fn an_answer_about_another_folder_or_another_backup_is_not_shown() {
        // A measurement is on its way when the cache moves: its answer is
        // about the old folder, and the new one is measured for itself.
        let mut state = ready();
        let first = panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        assert!(matches!(first, Effect::MeasureCache { asked: 1, .. }));
        let folder = PathBuf::from("E:/pid cache");
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Folder(Some(folder.clone())))),
            Effect::MeasureCache {
                asked: 3,
                root: folder.clone(),
                current: Some(PathBuf::from("D:/tmp/restore")),
            }
        );
        assert_eq!(
            panel(
                &mut state,
                cache(CacheMsg::Measured {
                    asked: 1,
                    result: Ok(usage(9, 9000)),
                })
            ),
            Effect::None
        );
        assert_eq!(cache_usage(&state), None);
        assert!(shown(&state).contains(&note(t!("Measuring the cache…"))));
        answer(
            &mut state,
            Ok(CacheUsage {
                root: folder.clone(),
                others: 1,
                other_bytes: 2048,
                ..CacheUsage::default()
            }),
        );
        assert_eq!(cache_usage(&state).map(|usage| usage.others), Some(1));

        // A backup goes while its cache is measured: the answer is not the
        // next backup's, which measures when its row is selected.
        let mut state = ready();
        panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        let index = state.index().expect("the backup is ready").clone();
        panel(&mut state, PidBackupMsg::Loaded(Ok(index)));
        assert_eq!(
            panel(
                &mut state,
                cache(CacheMsg::Measured {
                    asked: 1,
                    result: Ok(usage(9, 9000)),
                })
            ),
            Effect::None
        );
        assert_eq!(cache_usage(&state), None);
        assert!(matches!(
            panel(&mut state, PidBackupMsg::Select(NodeId::Root)),
            Effect::MeasureCache { asked: 3, .. }
        ));

        // The cache moves while it is cleared: what was freed is still
        // news, and the folder now on screen is measured once the clearing
        // is over.
        let mut state = measured(usage(2, 4096));
        panel(&mut state, cache(CacheMsg::Clear));
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Folder(Some(folder.clone())))),
            Effect::None
        );
        let _ = take_cache_outcome(&mut state);
        assert_eq!(
            panel(
                &mut state,
                cache(CacheMsg::Measured {
                    asked: 2,
                    result: Ok(CacheUsage {
                        freed: Some((2, 4096)),
                        ..usage(0, 0)
                    }),
                })
            ),
            Effect::MeasureCache {
                asked: 4,
                root: folder.clone(),
                current: Some(PathBuf::from("D:/tmp/restore")),
            }
        );
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!("{} restore(s) removed, {} freed", 2, "4.0 KB").into_owned()))
        );
    }

    #[test]
    fn the_command_line_is_told_what_the_cache_holds_when_it_asks() {
        // With the backup's restore in the cache.
        let mut state = ready();
        assert!(matches!(
            panel(&mut state, cache(CacheMsg::Report)),
            Effect::MeasureCache { asked: 1, .. }
        ));
        assert_eq!(take_cache_outcome(&mut state), None);
        answer(&mut state, Ok(usage(2, 211_812_352)));
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!(
                "the restore cache {} holds this backup's restore of {} and {} other restore(s) of {} together",
                tf!("{} (the default folder)", default_cache_root().display()),
                "160.2 MB",
                2,
                "202.0 MB"
            )
            .into_owned()))
        );
        // A measurement nobody asked the command line about says nothing
        // there.
        panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        answer(&mut state, Ok(usage(2, 211_812_352)));
        assert_eq!(take_cache_outcome(&mut state), None);

        // With no backup open: every restore in the folder.
        let mut state = PidBackupBrowser::default();
        assert_eq!(
            panel(&mut state, cache(CacheMsg::Report)),
            Effect::MeasureCache {
                asked: 1,
                root: default_cache_root(),
                current: None,
            }
        );
        answer(
            &mut state,
            Ok(CacheUsage {
                root: default_cache_root(),
                others: 3,
                other_bytes: 1_048_576,
                ..CacheUsage::default()
            }),
        );
        assert_eq!(
            take_cache_outcome(&mut state),
            Some(Ok(tf!(
                "the restore cache {} holds {} restore(s) of {} together",
                tf!("{} (the default folder)", default_cache_root().display()),
                3,
                "1.0 MB"
            )
            .into_owned()))
        );
        // Asked while a measurement is on its way: that one's answer is the
        // report.
        let mut state = ready();
        panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        assert_eq!(panel(&mut state, cache(CacheMsg::Report)), Effect::None);
        answer(&mut state, Ok(usage(0, 0)));
        assert!(matches!(take_cache_outcome(&mut state), Some(Ok(_))));
    }

    #[test]
    fn a_folder_read_where_it_lies_says_that_nothing_is_cached() {
        let mut state = ready();
        let mut index = (**state.index().expect("the backup is ready")).clone();
        index.restored = index.source.clone();
        panel(&mut state, PidBackupMsg::Loaded(Ok(std::sync::Arc::new(index))));
        panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        answer(
            &mut state,
            Ok(CacheUsage {
                root: default_cache_root(),
                others: 1,
                other_bytes: 2048,
                ..CacheUsage::default()
            }),
        );
        assert_eq!(
            shown(&state)[1..3],
            [
                this_backup(t!("the folder is read where it lies; nothing is cached")),
                others(tf!("{}, {} together", 1, "2.0 KB")),
            ]
        );
    }

    /// The buttons as the panel draws them: each is where a click does what
    /// it names.
    #[test]
    fn the_drawn_cache_buttons_clear_the_cache_and_ask_for_a_folder() {
        let _guard = crate::headless_ui_lock();
        let mut state = ready();
        // The details are long; give them most of the panel.
        panel(&mut state, PidBackupMsg::SetSplit(0.3));
        panel(&mut state, cache(CacheMsg::Folder(Some(PathBuf::from("E:/pid cache")))));
        panel(&mut state, PidBackupMsg::Select(NodeId::Root));
        answer(
            &mut state,
            Ok(CacheUsage {
                root: PathBuf::from("E:/pid cache"),
                others: 1,
                other_bytes: 4096,
                ..CacheUsage::default()
            }),
        );
        let click = |label: std::borrow::Cow<'static, str>| -> Vec<PidBackupMsg> {
            let mut ui = iced_test::simulator::Simulator::with_size(
                iced::Settings::default(),
                (340.0, 700.0),
                view(&state, 340.0, Chrome::default()),
            );
            let bounds = ui
                .find(label.as_ref())
                .unwrap_or_else(|error| panic!("{label} is drawn: {error:?}"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label} is on screen"));
            ui.point_at(bounds.center());
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages()
                .filter_map(|message| match message {
                    Message::PidBackup(message) => Some(message),
                    _ => None,
                })
                .collect()
        };
        let pressed = click(t!("Clear the other restores"));
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Cache(CacheMsg::Clear))),
            "{pressed:?}"
        );
        let pressed = click(t!("Change the cache folder…"));
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Cache(CacheMsg::PickFolder))),
            "{pressed:?}"
        );
        let pressed = click(t!("Use the default folder"));
        assert!(
            pressed
                .iter()
                .any(|message| matches!(message, PidBackupMsg::Cache(CacheMsg::Folder(None)))),
            "{pressed:?}"
        );
    }
}
