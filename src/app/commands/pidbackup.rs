//! PIDBACKUPOPEN -- open a drawing out of a SmartPlant P&ID backup
//! (`io::pid_backup`).
//!
//! ```text
//! PIDBACKUPOPEN                      prompt [Zip/Folder] <Zip>, then show the
//!                                    matching file or folder dialog
//! PIDBACKUPOPEN <source> [<folder>]  restore <source> (a backup zip or
//!                                    folder), into <folder> if given
//! ```
//!
//! The restore runs on a worker thread. When it is done the command line
//! says where the drawings are and how many; one drawing opens at once,
//! several open a `.pid` dialog in their folder, and the pick goes through
//! the ordinary open. With FILEDIA 0 or under a script nothing is shown: the
//! source is asked for on the command line and the drawings are left to OPEN.

use super::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
fn unquoted(arg: &str) -> &str {
    let quoted = arg.len() >= 2
        && ((arg.starts_with('"') && arg.ends_with('"'))
            || (arg.starts_with('\'') && arg.ends_with('\'')));
    if quoted {
        &arg[1..arg.len() - 1]
    } else {
        arg
    }
}

/// What the words after PIDBACKUPOPEN ask for.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq)]
enum Request {
    Zip,
    Folder,
    Restore(PathBuf, Option<PathBuf>),
}

/// Split `<source> [<folder>]`. A line that names an existing path whole is
/// the source alone, so an unquoted path with spaces still works; otherwise
/// the words split at whitespace outside quotes.
#[cfg(not(target_arch = "wasm32"))]
fn parse_request(rest: &str, exists: impl Fn(&Path) -> bool) -> Option<Request> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    if ["Z", "ZIP"]
        .iter()
        .any(|word| rest.eq_ignore_ascii_case(word))
    {
        return Some(Request::Zip);
    }
    if ["F", "FOLDER"]
        .iter()
        .any(|word| rest.eq_ignore_ascii_case(word))
    {
        return Some(Request::Folder);
    }
    let whole = unquoted(rest);
    if exists(Path::new(whole)) {
        return Some(Request::Restore(PathBuf::from(whole), None));
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for ch in rest.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => word.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            None => word.push(ch),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let mut words = words.into_iter();
    let source = PathBuf::from(words.next()?);
    let rest: Vec<String> = words.collect();
    let target = (!rest.is_empty()).then(|| PathBuf::from(rest.join(" ")));
    Some(Request::Restore(source, target))
}

impl OpenCADStudio {
    pub(super) fn dispatch_pidbackup(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("PIDBACKUPOPEN")?;
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        Some(self.start_pid_backup(cmd, rest, i))
    }

    #[cfg(target_arch = "wasm32")]
    fn start_pid_backup(&mut self, _cmd: &str, _rest: &str, _i: usize) -> Task<Message> {
        self.command_line.push_error(
            crate::t!(
                "PIDBACKUPOPEN: opening a SmartPlant backup is not available in the web build."
            )
            .as_ref(),
        );
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_pid_backup(&mut self, cmd: &str, rest: &str, i: usize) -> Task<Message> {
        let on_command_line = self.file_prompts_on_command_line();
        if rest.trim().is_empty() {
            // Bare verb: ask. Enter on the prompt comes back with a trailing
            // space and takes the default.
            if cmd.trim_end() == cmd {
                use crate::command::FreeTextValuePromptCommand;
                let c = if on_command_line {
                    FreeTextValuePromptCommand::new(
                        "PIDBACKUPOPEN",
                        "PIDBACKUPOPEN  Backup zip file or folder:",
                    )
                } else {
                    FreeTextValuePromptCommand::new(
                        "PIDBACKUPOPEN",
                        "PIDBACKUPOPEN  Backup source [Zip/Folder] <Zip>:",
                    )
                };
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
                return self.finish_dispatch(cmd);
            }
            if on_command_line {
                return Task::none();
            }
            return pick_backup_source(false);
        }
        match parse_request(rest, Path::exists) {
            Some(Request::Restore(source, target)) => self.restore_pid_backup(source, target),
            Some(Request::Zip | Request::Folder) if on_command_line => {
                // No dialogs here: the keyword is not a path either.
                self.command_line
                    .push_info(crate::t!("PIDBACKUPOPEN  Backup zip file or folder:").as_ref());
                Task::none()
            }
            Some(Request::Zip) => pick_backup_source(false),
            Some(Request::Folder) => pick_backup_source(true),
            None => Task::none(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn restore_pid_backup(&mut self, source: PathBuf, target: Option<PathBuf>) -> Task<Message> {
        // A relative restore folder sits beside the backup.
        let target = target.map(|target| match source.parent() {
            Some(parent) if target.is_relative() => parent.join(target),
            _ => target,
        });
        self.command_line
            .push_output(crate::tf!("PIDBACKUPOPEN: restoring {} ...", source.display()).as_ref());
        Task::perform(
            crate::io::pid_backup::restore_in_background(source, target),
            Message::PidBackupRestored,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_pid_backup_source_picked(
        &mut self,
        source: Option<PathBuf>,
    ) -> Task<Message> {
        match source {
            Some(source) => self.restore_pid_backup(source, None),
            None => Task::none(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_pid_backup_restored(
        &mut self,
        result: Result<crate::io::pid_backup::Restored, String>,
    ) -> Task<Message> {
        let restored = match result {
            Ok(restored) => restored,
            Err(error) => {
                self.command_line
                    .push_error(crate::tf!("PIDBACKUPOPEN: {}", error).as_ref());
                return Task::none();
            }
        };
        if restored.drawings.is_empty() {
            self.command_line.push_error(
                crate::tf!(
                    "PIDBACKUPOPEN: no .pid drawing in {}.",
                    restored.root.display()
                )
                .as_ref(),
            );
            return Task::none();
        }
        if !restored.extracted {
            self.command_line.push_info(
                crate::t!("PIDBACKUPOPEN: the folder is used in place (read-only).").as_ref(),
            );
        } else if restored.reused {
            self.command_line.push_info(
                crate::tf!(
                    "PIDBACKUPOPEN: reused the earlier restore in {}.",
                    restored.root.display()
                )
                .as_ref(),
            );
        }
        self.command_line.push_output(
            crate::tf!(
                "PIDBACKUPOPEN: {} .pid drawing(s) in {}",
                restored.drawings.len(),
                restored.root.display()
            )
            .as_ref(),
        );
        if let [only] = restored.drawings.as_slice() {
            return Task::done(Message::OpenRecent(only.clone()));
        }
        let folder = restored.drawing_folder();
        if self.file_prompts_on_command_line() {
            self.command_line.push_info(
                crate::tf!(
                    "PIDBACKUPOPEN: open one with OPEN, from {}.",
                    folder.display()
                )
                .as_ref(),
            );
            return Task::none();
        }
        Task::perform(
            async move {
                crate::sys::file_dialog()
                    .set_directory(folder)
                    .set_title(crate::t!("Select P&ID Drawing").as_ref())
                    .add_filter(crate::t!("P&ID drawing (*.pid)").as_ref(), &["pid", "PID"])
                    .pick_file()
                    .await
                    .map(|handle| crate::sys::handle_path(&handle))
            },
            Message::PidBackupDrawingPicked,
        )
    }
}

/// The dialog for the backup itself: a zip file, or with `folder` a folder.
#[cfg(not(target_arch = "wasm32"))]
fn pick_backup_source(folder: bool) -> Task<Message> {
    Task::perform(
        async move {
            let dialog = crate::sys::file_dialog();
            let handle = if folder {
                dialog
                    .set_title(crate::t!("Select P&ID Backup Folder").as_ref())
                    .pick_folder()
                    .await
            } else {
                dialog
                    .set_title(crate::t!("Select P&ID Backup").as_ref())
                    .add_filter(
                        crate::t!("SmartPlant backup (*.zip)").as_ref(),
                        &["zip", "ZIP"],
                    )
                    .pick_file()
                    .await
            };
            handle.map(|handle| crate::sys::handle_path(&handle))
        },
        Message::PidBackupSourcePicked,
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    #[test]
    fn keywords_pick_the_dialog() {
        assert_eq!(parse_request("z", none), Some(Request::Zip));
        assert_eq!(parse_request(" Zip ", none), Some(Request::Zip));
        assert_eq!(parse_request("f", none), Some(Request::Folder));
        assert_eq!(parse_request("FOLDER", none), Some(Request::Folder));
        assert_eq!(parse_request("  ", none), None);
    }

    #[test]
    fn a_source_and_a_restore_folder_split_at_whitespace_outside_quotes() {
        assert_eq!(
            parse_request(r#""D:/my backups/a.zip" D:/out"#, none),
            Some(Request::Restore(
                PathBuf::from("D:/my backups/a.zip"),
                Some(PathBuf::from("D:/out"))
            ))
        );
        assert_eq!(
            parse_request("D:/a.zip", none),
            Some(Request::Restore(PathBuf::from("D:/a.zip"), None))
        );
    }

    #[test]
    fn an_existing_path_with_spaces_is_the_source_alone() {
        let exists = |path: &Path| path == Path::new("D:/my backups/plant");
        assert_eq!(
            parse_request("D:/my backups/plant", exists),
            Some(Request::Restore(PathBuf::from("D:/my backups/plant"), None))
        );
    }
}
