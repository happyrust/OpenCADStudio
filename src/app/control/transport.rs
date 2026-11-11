//! Private local discovery and a bounded GUI message queue.
use super::{session_id, Envelope, Reply};
use iced::futures::{channel::mpsc, Stream};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

pub(in crate::app) fn subscribe() -> iced::Subscription<Envelope> {
    iced::Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Envelope> {
    iced::stream::channel(32, |sender| async move {
        std::thread::spawn(move || {
            if let Err(error) = listen(sender) {
                eprintln!("Automation unavailable: {error}");
            }
        });
        iced::futures::future::pending::<()>().await;
    })
}

fn listen(sender: mpsc::Sender<Envelope>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let dir = crate::config::config_dir()
        .ok_or_else(|| std::io::Error::other("No user directory"))?
        .join("automation");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(std::io::Error::other)?;
    let token: String = secret.iter().map(|v| format!("{v:02x}")).collect();
    let descriptor = json!({"protocol":1,"session_id":session_id(),"pid":std::process::id(),"port":listener.local_addr()?.port(),"token":token,"executable":std::env::current_exe()?.to_string_lossy()});
    let published = Descriptor::publish(&dir, session_id(), &descriptor)?;
    if let Ok(mut slot) = PUBLISHED.lock() {
        *slot = Some(published);
    }
    let _ = crate::mcp::sync_agent_tool_schemas();
    let clients = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        // Replies are one-shot request/response lines: Nagle would hold each
        // reply's tail segment for the peer's delayed ACK (the bridge sends
        // nothing back until the full reply arrives). Disable coalescing on
        // both ends of the exchange.
        let _ = stream.set_nodelay(true);
        if clients.fetch_add(1, Ordering::SeqCst) >= 8 {
            clients.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let clients = clients.clone();
        let mut sender = sender.clone();
        let token = token.clone();
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
            if let Ok(write_half) = stream.try_clone() {
                let mut reader = BufReader::new(stream);
                let mut writer = write_half;
                // One bounded request per connection; operation polling reconnects.
                let mut line = String::new();
                if reader.by_ref().take(1_048_577).read_line(&mut line).is_ok()
                    && line.len() <= 1_048_576
                    && line.ends_with('\n')
                {
                    if let Ok(mut request) = serde_json::from_str::<Value>(&line) {
                        if request["token"].as_str() == Some(token.as_str()) {
                            if let Some(object) = request.as_object_mut() {
                                object.remove("token");
                            }
                            let (reply, response) = std::sync::mpsc::channel();
                            let result = if sender
                                .try_send(Envelope {
                                    request,
                                    reply: Reply::Native(reply),
                                })
                                .is_ok()
                            {
                                response.recv_timeout(Duration::from_secs(15)).unwrap_or_else(|_| json!({"ok":false,"code":"response_timeout","error":"Query operation status with the same request_id; execution may continue."}))
                            } else {
                                json!({"ok":false,"code":"busy","error":"GUI queue full"})
                            };
                            // write_fmt streams a Value Display token by
                            // token: one unbuffered TCP syscall per JSON
                            // token (tens of thousands for a records reply,
                            // ~20 us each on loopback = the dominant ocs_read
                            // cost). Serialize once, write once; the bytes
                            // are identical (Value's Display == to_string).
                            let mut reply_line = result.to_string();
                            reply_line.push('\n');
                            let _ = writer.write_all(reply_line.as_bytes());
                        }
                    }
                }
            }
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    retire_descriptor();
    Ok(())
}

/// The file this process published for its automation channel: where a
/// local client reads the port and the per-session token.
struct Descriptor {
    path: std::path::PathBuf,
}

impl Descriptor {
    /// Write `body` as `<dir>/<session_id>.json`. The file must not exist
    /// yet, and on Unix only its owner can read it: it carries the token.
    fn publish(dir: &std::path::Path, session_id: &str, body: &Value) -> std::io::Result<Self> {
        let path = dir.join(format!("{session_id}.json"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        write!(file, "{body}")?;
        Ok(Self { path })
    }

    /// Take the file away again.
    fn retire(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What [`listen`] published, for the way out to take away.
static PUBLISHED: std::sync::Mutex<Option<Descriptor>> = std::sync::Mutex::new(None);

/// Remove the descriptor this process published, if it published one.
///
/// Called as the application quits. The listener thread sits in `accept`
/// for as long as the process lives and never reaches the removal after its
/// loop, so every run used to leave its descriptor behind: a file naming a
/// port nobody listens on any more, with the token for it on its first line.
pub(in crate::app) fn retire_descriptor() {
    // Removed with the slot still locked: a second caller then finds the
    // file already gone rather than about to be.
    let Ok(mut slot) = PUBLISHED.lock() else {
        return;
    };
    if let Some(published) = slot.take() {
        published.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocs-automation-descriptor-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn body() -> Value {
        json!({"protocol": 1, "session_id": "s", "pid": 1, "port": 1, "token": "not-a-real-one"})
    }

    /// The descriptor carries the channel's token. Once it is retired the
    /// file is gone, not emptied or left for the next run to find.
    #[test]
    fn a_retired_descriptor_is_gone() {
        let dir = scratch("retired");
        let published = Descriptor::publish(&dir, "0123abcd", &body()).expect("published");
        let path = dir.join("0123abcd.json");
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the file is there"))
                .expect("it is the JSON it was given");
        assert_eq!(written["port"], 1);
        assert!(written["token"].is_string());

        published.retire();
        assert!(!path.exists(), "the descriptor outlived its retirement");
        assert_eq!(
            std::fs::read_dir(&dir).expect("dir").count(),
            0,
            "nothing else is left in its place"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A descriptor that is already there belongs to somebody: publishing
    /// over it is refused, and theirs is left as it was.
    #[test]
    fn publishing_does_not_overwrite_a_descriptor_already_there() {
        let dir = scratch("taken");
        let theirs = Descriptor::publish(&dir, "0123abcd", &body()).expect("first");
        let before = std::fs::read_to_string(dir.join("0123abcd.json")).expect("read");
        let again = Descriptor::publish(&dir, "0123abcd", &json!({"token": "another"}));
        assert!(again.is_err(), "a second descriptor replaced the first");
        assert_eq!(
            std::fs::read_to_string(dir.join("0123abcd.json")).expect("read"),
            before
        );
        theirs.retire();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Quitting takes the published descriptor away. This is the one test
    /// that puts something in the process-wide slot: a second one would
    /// replace what this one put there, and its file would never be removed.
    #[test]
    fn quitting_retires_the_descriptor_this_process_published() {
        let dir = scratch("quit");
        let published = Descriptor::publish(&dir, "0123abcd", &body()).expect("published");
        let path = dir.join("0123abcd.json");
        *PUBLISHED.lock().expect("slot") = Some(published);

        let mut app = crate::app::OpenCADStudio::new_for_test();
        let _ = app.exit_app();
        assert!(!path.exists(), "the application quit and left its descriptor");

        // Nothing published: quitting again has nothing to remove.
        let _ = app.exit_app();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
