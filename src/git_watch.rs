//! Shared, read-only plain-Git change detection. Native events are hints;
//! bounded event state and periodic metadata scans cover missed notifications.
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

const QUIET: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitStamp {
    pub generation: u64,
    pub references: u64,
}
#[derive(Default)]
struct Events {
    sequence: u64,
    forced: u64,
    last: Option<Instant>,
    files: HashSet<PathBuf>,
    metadata: Vec<PathBuf>,
}
impl Events {
    fn record(&mut self, event: notify::Result<Event>) {
        if event
            .as_ref()
            .is_ok_and(|event| matches!(event.kind, EventKind::Access(_)))
        {
            return;
        }
        self.sequence += 1;
        self.last = Some(Instant::now());
        let force = match event {
            Err(_) => true, // An overflow/error invalidates the captured view.
            Ok(event) => {
                event.need_rescan()
                    || event.paths.iter().any(|path| {
                        self.files.contains(path)
                            || self.metadata.iter().any(|root| {
                                path.strip_prefix(root).is_ok_and(|relative| {
                                    let name = relative.to_string_lossy();
                                    !name.ends_with(".lock")
                                        && (matches!(
                                            name.as_ref(),
                                            "HEAD"
                                                | "index"
                                                | "config"
                                                | "packed-refs"
                                                | "shallow"
                                                | "info/exclude"
                                        ) || name.starts_with("refs/"))
                                })
                            })
                    })
            }
        };
        if force {
            self.forced = self.sequence;
        }
    }
}
struct Snapshot {
    fingerprint: u64,
    references: u64,
    files: HashSet<PathBuf>,
    directories: BTreeSet<PathBuf>,
}
fn hash_metadata(path: &Path, hash: &mut impl Hasher) -> Result<(), String> {
    path.hash(hash);
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false.hash(hash),
        Err(error) => return Err(format!("Could not inspect {}: {error}", path.display())),
        Ok(metadata) => {
            true.hash(hash);
            metadata.len().hash(hash);
            metadata.file_type().is_symlink().hash(hash);
            metadata
                .modified()
                .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
                .hash(hash);
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                metadata.ino().hash(hash);
                metadata.ctime().hash(hash);
                metadata.ctime_nsec().hash(hash);
            }
        }
    }
    Ok(())
}
impl Snapshot {
    fn read(root: &Path, metadata: &[PathBuf]) -> Result<Self, String> {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let references = crate::git::references(root)?.fingerprint();
        references.hash(&mut hash);
        crate::git::output(
            root,
            &["status", "--porcelain", "-z", "--untracked-files=normal"],
        )?
        .hash(&mut hash);
        let inventory = crate::git::output(
            root,
            &[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ],
        )?;
        let names: BTreeSet<_> = inventory.split_terminator('\0').collect();
        let mut files = HashSet::new();
        let mut directories = BTreeSet::from([root.to_owned()]);
        for name in names {
            let path = root.join(name);
            // Register parents individually, excluding ignored directory trees
            // and never following directory symlinks outside the repository.
            let mut directory = root.to_owned();
            let mut reachable = true;
            if let Some(parent) = Path::new(name).parent() {
                for component in parent.components() {
                    directory.push(component);
                    match std::fs::symlink_metadata(&directory) {
                        Ok(kind) if kind.is_dir() && !kind.is_symlink() => {
                            directories.insert(directory.clone());
                        }
                        Ok(_) => {
                            reachable = false;
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            reachable = false;
                            break;
                        }
                        Err(error) => {
                            return Err(format!(
                                "Could not inspect {}: {error}",
                                directory.display()
                            ));
                        }
                    }
                }
            }
            if reachable {
                hash_metadata(&path, &mut hash)?;
            } else {
                path.hash(&mut hash);
                false.hash(&mut hash);
            }
            files.insert(path);
        }
        for directory in metadata {
            for name in [
                "HEAD",
                "index",
                "config",
                "packed-refs",
                "shallow",
                "info/exclude",
            ] {
                hash_metadata(&directory.join(name), &mut hash)?;
            }
        }
        Ok(Self {
            fingerprint: hash.finish(),
            references,
            files,
            directories,
        })
    }
}
struct Inner {
    watcher: Option<RecommendedWatcher>,
    roots: HashMap<PathBuf, RecursiveMode>,
    fingerprint: u64,
    stamp: GitStamp,
    handled: u64,
    scanned: Instant,
}
impl Inner {
    fn watch(&mut self, directories: BTreeSet<PathBuf>, metadata: &[PathBuf]) {
        let mut wanted: HashMap<_, _> = directories
            .into_iter()
            .map(|path| (path, RecursiveMode::NonRecursive))
            .collect();
        for directory in metadata {
            for path in [directory.clone(), directory.join("info")] {
                if path.is_dir() {
                    wanted.insert(path, RecursiveMode::NonRecursive);
                }
            }
            let refs = directory.join("refs");
            if refs.is_dir() {
                wanted.insert(refs, RecursiveMode::Recursive);
            }
        }
        if let Some(watcher) = &mut self.watcher {
            for path in self.roots.keys().filter(|path| !wanted.contains_key(*path)) {
                let _ = watcher.unwatch(path); // A deleted watch may already be gone.
            }
            for (path, mode) in wanted
                .iter()
                .filter(|(path, _)| !self.roots.contains_key(*path))
            {
                if watcher.watch(path, *mode).is_err() {
                    // Unsupported filesystems/watch limits use periodic scans.
                    self.watcher = None;
                    self.roots.clear();
                    return;
                }
            }
            self.roots = wanted;
        }
    }
}
pub struct GitWatch {
    root: PathBuf,
    metadata: Vec<PathBuf>,
    events: Arc<Mutex<Events>>,
    inner: Mutex<Inner>,
}
impl GitWatch {
    pub fn subscribe(root: &Path) -> Result<Arc<Self>, String> {
        static WATCHES: OnceLock<Mutex<HashMap<PathBuf, Weak<GitWatch>>>> = OnceLock::new();
        let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
        let mut watches = WATCHES
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "Git watcher registry lock poisoned")?;
        watches.retain(|_, watcher| watcher.strong_count() > 0);
        if let Some(watcher) = watches.get(&root).and_then(Weak::upgrade) {
            return Ok(watcher);
        }
        let metadata: BTreeSet<_> = ["--absolute-git-dir", "--git-common-dir"]
            .into_iter()
            .map(|argument| {
                crate::git::output(&root, &["rev-parse", "--path-format=absolute", argument])
                    .map(|path| PathBuf::from(path.trim_end_matches('\n')))
            })
            .collect::<Result<_, _>>()?;
        let metadata: Vec<_> = metadata.into_iter().collect();
        let snapshot = Snapshot::read(&root, &metadata)?;
        let events = Arc::new(Mutex::new(Events {
            files: snapshot.files,
            metadata: metadata.clone(),
            ..Default::default()
        }));
        let changes = events.clone();
        // A failed native backend deliberately selects the metadata-scan fallback.
        let watcher = notify::recommended_watcher(move |event| {
            if let Ok(mut changes) = changes.lock() {
                changes.record(event);
            }
        })
        .ok();
        let mut inner = Inner {
            watcher,
            roots: HashMap::new(),
            fingerprint: snapshot.fingerprint,
            stamp: GitStamp {
                generation: 1,
                references: snapshot.references,
            },
            handled: 0,
            scanned: Instant::now(),
        };
        inner.watch(snapshot.directories, &metadata);
        let watcher = Arc::new(Self {
            root: root.clone(),
            metadata,
            events,
            inner: Mutex::new(inner),
        });
        watches.insert(root, Arc::downgrade(&watcher));
        Ok(watcher)
    }
    /// Call off the UI thread. Shared scans serialize and coalesce across windows.
    pub fn poll(&self, fallback: Duration) -> Result<GitStamp, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Git watcher state lock poisoned")?;
        let (sequence, forced, last) = {
            let events = self
                .events
                .lock()
                .map_err(|_| "Git watcher event lock poisoned")?;
            (events.sequence, events.forced, events.last)
        };
        let unsettled = last.is_some_and(|last| last.elapsed() < QUIET);
        let dirty = sequence != inner.handled;
        let due = inner.scanned.elapsed() >= fallback;
        if !due && (unsettled || !dirty) {
            return Ok(inner.stamp);
        }
        let snapshot = Snapshot::read(&self.root, &self.metadata)?;
        if snapshot.fingerprint != inner.fingerprint || forced > inner.handled {
            inner.stamp.generation += 1;
            inner.stamp.references = snapshot.references;
        }
        inner.fingerprint = snapshot.fingerprint;
        inner.handled = sequence;
        inner.scanned = Instant::now();
        self.events
            .lock()
            .map_err(|_| "Git watcher event lock poisoned")?
            .files = snapshot.files;
        inner.watch(snapshot.directories, &self.metadata);
        Ok(inner.stamp)
    }
}
impl Drop for GitWatch {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut()
            && let Some(watcher) = inner.watcher.take()
        {
            // Native watcher shutdown can join a platform thread. Keep that
            // cleanup off the UI thread when the last viewer releases it.
            std::thread::spawn(move || drop(watcher));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_refresh::GitRefresh;
    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "jaywalk-watch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let repo = root.join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            let f = Self { root, repo };
            f.git(&["init", "-b", "main"]);
            f.write("file.txt", "original\n");
            f.write(".gitignore", "build/\n");
            f.git(&["add", "."]);
            f.git(&["commit", "-m", "initial"]);
            f
        }
        fn git(&self, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
                .current_dir(&self.repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_OPTIONAL_LOCKS", "0")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim_end().into()
        }
        fn write(&self, path: &str, text: &str) {
            let path = self.repo.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        fn polling(&self) -> Arc<GitWatch> {
            let watch = GitWatch::subscribe(&self.repo).unwrap();
            drop(watch.inner.lock().unwrap().watcher.take());
            let sequence = watch.events.lock().unwrap().sequence;
            watch.inner.lock().unwrap().handled = sequence;
            watch
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn poll(watch: &GitWatch) -> GitStamp {
        watch.poll(Duration::ZERO).unwrap()
    }

    #[test]
    fn shared_polling_detects_repeated_edits_atomic_saves_renames_and_deletion() {
        let f = Fixture::new();
        let watch = f.polling();
        let alias = GitWatch::subscribe(&f.repo.join(".")).unwrap();
        assert!(Arc::ptr_eq(&watch, &alias));
        let initial = poll(&watch);
        f.write("file.txt", "first\n");
        let first = poll(&watch);
        assert!(first.generation > initial.generation);
        f.write("file.txt", "other\n");
        let second = poll(&watch);
        assert!(second.generation > first.generation);
        assert_eq!(poll(&alias), second);
        f.write("save.tmp", "atomic\n");
        std::fs::rename(f.repo.join("save.tmp"), f.repo.join("file.txt")).unwrap();
        let saved = poll(&watch);
        assert!(saved.generation > second.generation);
        std::fs::create_dir(f.repo.join("nested")).unwrap();
        std::fs::rename(f.repo.join("file.txt"), f.repo.join("nested/renamed.txt")).unwrap();
        let renamed = poll(&watch);
        assert!(renamed.generation > saved.generation);
        std::fs::remove_file(f.repo.join("nested/renamed.txt")).unwrap();
        assert!(poll(&watch).generation > renamed.generation);
    }
    #[test]
    fn ignore_rules_and_readonly_inspection_do_not_cause_refresh_loops() {
        let f = Fixture::new();
        let watch = f.polling();
        let initial = poll(&watch);
        let head = f.git(&["rev-parse", "HEAD"]);
        let status = f.git(&["status", "--porcelain"]);
        let index = std::fs::metadata(f.repo.join(".git/index"))
            .unwrap()
            .modified()
            .unwrap();
        f.write("build/output", "ignored\n");
        assert_eq!(poll(&watch), initial);
        assert_eq!(poll(&watch), initial);
        assert_eq!(f.git(&["status", "--porcelain"]), status);
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            std::fs::metadata(f.repo.join(".git/index"))
                .unwrap()
                .modified()
                .unwrap(),
            index
        );
    }
    #[test]
    fn staging_changes_and_linked_worktree_metadata_are_observed() {
        let f = Fixture::new();
        let linked = f.root.join("linked");
        f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
        let watch = GitWatch::subscribe(&linked).unwrap();
        drop(watch.inner.lock().unwrap().watcher.take());
        assert_eq!(watch.metadata.len(), 2);
        let original = poll(&watch);
        std::fs::write(linked.join("file.txt"), "modified\n").unwrap();
        let modified = poll(&watch);
        assert!(modified.generation > original.generation);
        let result = std::process::Command::new("git")
            .current_dir(&linked)
            .args(["add", "file.txt"])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(poll(&watch).generation > modified.generation);
    }
    #[test]
    fn refresh_retains_the_requested_review_and_the_virtual_working_copy() {
        let f = Fixture::new();
        let before = f.git(&["rev-parse", "HEAD"]);
        f.write("file.txt", "committed\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-m", "next"]);
        let tip = f.git(&["rev-parse", "HEAD"]);
        f.write("new.txt", "untracked\n");
        let refresh = GitRefresh::load(
            &f.repo,
            Some(&before),
            &tip,
            true,
            &Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(refresh.home.base(), before);
        assert_eq!(refresh.home.tip(), tip);
        assert!(refresh.revision.unwrap().is_virtual_working_copy());
        assert_eq!(refresh.comparison.files().len(), 1);
        assert_eq!(refresh.comparison.files()[0].after_path(), "new.txt");
        assert_eq!(refresh.comparison.base(), tip);
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn native_events_refresh_before_the_fallback_interval() {
        let f = Fixture::new();
        let watch = GitWatch::subscribe(&f.repo).unwrap();
        assert!(watch.inner.lock().unwrap().watcher.is_some());
        let initial = watch.poll(Duration::from_secs(60)).unwrap();
        f.write("file.txt", "changed\n");
        let start = Instant::now();
        loop {
            if watch.poll(Duration::from_secs(60)).unwrap().generation > initial.generation {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "native save event was not observed"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
