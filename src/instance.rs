//! One Unix process owns the app; clients send requests and wait per window.
use crate::Arguments;
use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST: u64 = 1_048_576;

pub(crate) struct Request {
    pub(crate) arguments: Arguments,
    pub(crate) reply: mpsc::Sender<Response>,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum Response {
    Opened,
    Closed,
    Error(String),
}

pub(crate) fn directory() -> Result<PathBuf, String> {
    let config = jaywalk::settings::default_path()?;
    Ok(config.with_file_name("runtime"))
}

/// Keep the lock inode in place, including across process exits. Unlinking it
/// would allow two processes to acquire different files bearing the same name.
pub(crate) struct Server {
    #[cfg(unix)]
    listener: UnixListener,
    #[cfg(unix)]
    _lock: std::fs::File,
    socket: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}
impl Server {
    #[cfg(unix)]
    pub(crate) fn claim(directory: &Path) -> Result<Option<Self>, String> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not protect {}: {error}", directory.display()))?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join("instance.lock"))
            .map_err(|error| format!("Could not open instance lock: {error}"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(error) => return Err(format!("Could not lock the app instance: {error}")),
        }
        let socket = directory.join("instance.sock");
        match std::fs::remove_file(&socket) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not remove stale instance endpoint: {error}")),
        }
        let listener = UnixListener::bind(&socket)
            .map_err(|error| format!("Could not bind {}: {error}", socket.display()))?;
        Ok(Some(Self {
            listener,
            _lock: lock,
            socket,
        }))
    }

    #[cfg(not(unix))]
    pub(crate) fn claim(_: &Path) -> Result<Option<Self>, String> {
        Err("Single-instance transport is not implemented on this platform yet.".into())
    }

    pub(crate) fn start(self) -> Receiver<Request> {
        let (sender, receiver) = async_channel::unbounded();
        #[cfg(unix)]
        std::thread::spawn(move || {
            // Move the entire guard into this thread, not just its listener:
            // closure field capture must not drop the process lock early.
            let server = self;
            for connection in server.listener.incoming() {
                match connection {
                    Ok(stream) => {
                        let sender = sender.clone();
                        std::thread::spawn(move || serve(stream, sender));
                    }
                    Err(error) => eprintln!("Could not accept a launch request: {error}"),
                }
            }
        });
        #[cfg(not(unix))]
        drop(sender);
        receiver
    }
}

#[cfg(unix)]
fn serve(mut stream: UnixStream, sender: Sender<Request>) {
    let result = (|| -> Result<(), String> {
        stream
            .set_read_timeout(Some(STARTUP_TIMEOUT))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(STARTUP_TIMEOUT))
            .map_err(|error| error.to_string())?;
        let mut line = String::new();
        // Limit untrusted framing before allocating an arbitrary payload.
        std::io::Read::take(BufReader::new(&stream), MAX_REQUEST + 1)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if line.len() as u64 > MAX_REQUEST || !line.ends_with('\n') {
            return Err("Invalid launch request framing.".into());
        }
        let arguments: Arguments = serde_json::from_str(&line)
            .map_err(|error| format!("Invalid launch request: {error}"))?;
        let (reply, responses) = mpsc::channel();
        sender
            .send_blocking(Request { arguments, reply })
            .map_err(|_| "The app is shutting down".to_owned())?;
        for response in responses {
            serde_json::to_writer(&mut stream, &response).map_err(|error| error.to_string())?;
            stream.write_all(b"\n").map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = serde_json::to_writer(&mut stream, &Response::Error(error));
        let _ = stream.write_all(b"\n");
    }
}

/// None means no listening process; connection failures after connecting are
/// errors, never permission to create an independent second application.
#[cfg(unix)]
pub(crate) fn forward(directory: &Path, arguments: &Arguments) -> Result<bool, String> {
    let mut stream = match UnixStream::connect(directory.join("instance.sock")) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(format!("Could not contact Jaywalk: {error}")),
    };
    stream
        .set_read_timeout(Some(STARTUP_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(STARTUP_TIMEOUT))
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut stream, arguments)
        .map_err(|error| format!("Could not encode launch paths: {error}"))?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut opened = false;
    loop {
        let mut line = String::new();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| format!("Could not read Jaywalk's response: {error}"))?;
        if count == 0 {
            // Quitting the app releases every waiting client as well as closing
            // an individual window. EOF before acknowledgment is a failure.
            return if opened {
                Ok(true)
            } else {
                Err("Jaywalk exited before opening the requested window.".into())
            };
        }
        match serde_json::from_str(&line)
            .map_err(|error| format!("Invalid Jaywalk response: {error}"))?
        {
            Response::Opened if !arguments.wait => return Ok(true),
            Response::Opened => {
                opened = true;
                reader
                    .get_ref()
                    .set_read_timeout(None)
                    .map_err(|error| error.to_string())?;
            }
            Response::Closed if opened => return Ok(true),
            Response::Closed => return Err("Jaywalk closed the request before opening it.".into()),
            Response::Error(error) => return Err(error),
        }
    }
}
#[cfg(not(unix))]
pub(crate) fn forward(_: &Path, _: &Arguments) -> Result<bool, String> {
    Err("Single-instance transport is not implemented on this platform yet.".into())
}

pub(crate) fn wait_for_server(directory: &Path, arguments: &Arguments) -> Result<(), String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if forward(directory, arguments)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "Jaywalk did not start within 10 seconds. Check its log or run with --foreground."
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Response, Server, forward};
    use crate::Arguments;
    use clap::Parser;
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jw-ipc-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exclusive_owner_reclaims_stale_socket_only_after_the_lock_is_released() {
        let fixture = Fixture::new();
        let server = Server::claim(&fixture.0).unwrap().unwrap();
        assert!(Server::claim(&fixture.0).unwrap().is_none());
        drop(server);
        std::fs::write(fixture.0.join("instance.sock"), "stale endpoint").unwrap();
        let replacement = Server::claim(&fixture.0).unwrap().unwrap();
        assert!(Server::claim(&fixture.0).unwrap().is_none());
        drop(replacement);
    }

    #[test]
    fn wait_client_is_released_independently_and_does_not_block_another_launch() {
        let fixture = Fixture::new();
        let receiver = Server::claim(&fixture.0).unwrap().unwrap().start();
        // Covers accidentally capturing only the listener and dropping the lock.
        assert!(Server::claim(&fixture.0).unwrap().is_none());
        let (finished, result) = mpsc::channel();
        let directory = fixture.0.clone();
        std::thread::spawn(move || {
            let arguments = Arguments::parse_from(["jaywalk", "--wait"]);
            finished.send(forward(&directory, &arguments)).unwrap();
        });
        let waiting = receiver.recv_blocking().unwrap();
        waiting.reply.send(Response::Opened).unwrap();
        assert!(matches!(
            result.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        let directory = fixture.0.clone();
        let other =
            std::thread::spawn(move || forward(&directory, &Arguments::parse_from(["jaywalk"])));
        let launched = receiver.recv_blocking().unwrap();
        launched.reply.send(Response::Opened).unwrap();
        assert!(other.join().unwrap().unwrap());
        assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
        waiting.reply.send(Response::Closed).unwrap();
        assert!(
            result
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap()
        );
    }
}
