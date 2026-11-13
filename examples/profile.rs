//! Opt-in synthetic profiling. Never opens a native window or reads a user's repository.
use clap::Parser;
use gpui_kit::component::highlighter::PreparedInputHighlighter;
use jaywalk::{
    diff_model::FileDiff,
    fluid::{DiffGeometry, FluidScroll},
    history::HistoryQuery,
    repository::Comparison,
};
use std::{hint::black_box, io::Write, path::PathBuf, process::Command, time::Instant};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 3)]
    samples: usize,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=50))]
    large_mib: u32,
    /// Include a disposable jj operation/rewrite history fixture.
    #[arg(long)]
    jj: bool,
    /// Include 700 distinct branch tips sharing one parent, beyond the default history limit.
    #[arg(long)]
    wide_history: bool,
}

struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str) -> Result<Self, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let fixture = Self(std::env::temp_dir().join(format!(
            "jaywalk-profile-{label}-{}-{stamp}",
            std::process::id()
        )));
        std::fs::create_dir(&fixture.0).map_err(|e| e.to_string())?;
        fixture.run("git", &["init", "-b", "main"])?;
        Ok(fixture)
    }
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Profile Fixture")
            .env("GIT_AUTHOR_EMAIL", "profile@example.invalid")
            .env("GIT_COMMITTER_NAME", "Profile Fixture")
            .env("GIT_COMMITTER_EMAIL", "profile@example.invalid");
        command
    }
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        let output = self
            .command(program)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{program} {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        std::fs::write(self.0.join(path), text).map_err(|e| e.to_string())
    }
    fn commit(&self, message: &str) -> Result<(), String> {
        self.run("git", &["add", "."])?;
        self.run("git", &["commit", "-m", message])?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "Could not remove profiling fixture {}: {error}",
                self.0.display()
            );
        }
    }
}

fn resident_kib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|value| value.split_whitespace().next()?.parse().ok())
            })
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn measure<T>(
    name: &str,
    samples: usize,
    mut operation: impl FnMut() -> Result<T, String>,
) -> Result<(), String> {
    let resident_before = resident_kib();
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let value = operation()?;
        times.push(started.elapsed().as_secs_f64() * 1000.);
        black_box(value); // Drop outside the operation timer.
    }
    println!(
        "{}",
        serde_json::json!({"metric":name,"samples_ms":times,"rss_before_kib":resident_before,"rss_after_kib":resident_kib()})
    );
    Ok(())
}

fn text(rows: usize) -> String {
    (0..rows)
        .map(|row| format!("let value_{row} = process({row}); // synthetic source λ\n"))
        .collect()
}

fn run(args: Args) -> Result<(), String> {
    if args.samples == 0 || args.samples > 100 {
        return Err("samples must be between 1 and 100".into());
    }
    println!(
        "{}",
        serde_json::json!({"profile":"synthetic-domain","version":jaywalk::version::DISPLAY,"revision":jaywalk::version::REVISION,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"optimized":!cfg!(debug_assertions),"samples":args.samples,"large_mib":args.large_mib})
    );
    for rows in [200, 20_000, args.large_mib as usize * 1024 * 1024 / 58] {
        let mut before = text(rows);
        let maximum = args.large_mib as usize * 1024 * 1024;
        if before.len() > maximum {
            let mut boundary = maximum;
            while !before.is_char_boundary(boundary) {
                boundary -= 1;
            }
            before.truncate(boundary);
        }
        let rows = before.lines().count();
        let after = before.replace("process(100)", "updated(100)");
        measure(
            &format!("diff/{rows}-lines/{}-bytes", before.len()),
            args.samples,
            || Ok(FileDiff::new(&before, &after)),
        )?;
        measure(&format!("syntax/rust/{rows}-lines"), args.samples, || {
            Ok(PreparedInputHighlighter::new("rust", &after))
        })?;
        let full_patch = || {
            similar::TextDiff::from_lines(&before, &after)
                .unified_diff()
                .context_radius(before.lines().count().max(after.lines().count()))
                .header("a/source", "b/source")
                .to_string()
        };
        measure(
            &format!("standard/full-patch/{rows}-lines"),
            args.samples,
            || Ok(full_patch()),
        )?;
        let patch = full_patch();
        measure(
            &format!("standard/parse/{rows}-lines"),
            args.samples,
            || {
                gpui_kit::component::diff::DiffFile::parse(&patch)
                    .map_err(|error| error.to_string())
            },
        )?;
        let diff = FileDiff::new(&before, &after);
        let geometry = DiffGeometry {
            before: (0..diff.before_offsets.len())
                .map(|i| i as f32 * 20.)
                .collect(),
            after: (0..diff.after_offsets.len())
                .map(|i| i as f32 * 20.)
                .collect(),
            before_height: diff.before_offsets.len() as f32 * 20.,
            after_height: diff.after_offsets.len() as f32 * 20.,
        };
        measure(
            &format!("fluid/100-scroll-steps/{rows}-lines"),
            args.samples,
            || {
                let mut scroll = FluidScroll::default();
                for _ in 0..100 {
                    black_box(scroll.advance(120., &diff, &geometry, 800.));
                    black_box(geometry.scroll_extent(&diff));
                    black_box(geometry.change_spans(&diff).count());
                }
                Ok(scroll.position)
            },
        )?;
    }
    let large = Fixture::new("large-capture")?;
    let bytes = args.large_mib as usize * 1024 * 1024;
    large.write("large.txt", &"x".repeat(bytes - 1))?;
    large.commit("large baseline")?;
    large.write("large.txt", &"y".repeat(bytes - 1))?;
    measure(
        &format!("git/capture/{bytes}-byte-limit"),
        args.samples,
        || {
            Comparison::load_with_limit(
                &large.0,
                Some("HEAD"),
                "@",
                jaywalk::text_content::TextLimit::from_mib(args.large_mib as u64)?,
            )
        },
    )?;
    for files in [1, 64] {
        let f = Fixture::new("files")?;
        for index in 0..files {
            f.write(&format!("file-{index}.rs"), &text(200))?;
        }
        f.commit("baseline")?;
        for index in 0..files {
            f.write(
                &format!("file-{index}.rs"),
                &text(200).replace("process(100)", "updated(100)"),
            )?;
        }
        measure(
            &format!("git/open-working/{files}-files"),
            args.samples,
            || Comparison::load(&f.0, Some("HEAD"), "@"),
        )?;
        let comparison = Comparison::load(&f.0, Some("HEAD"), "@")?;
        measure(&format!("overview/{files}-files"), args.samples, || {
            Ok(comparison.overview())
        })?;
        measure(
            &format!("git/repeated-refresh/{files}-files"),
            args.samples,
            || {
                for _ in 0..3 {
                    black_box(Comparison::load(&f.0, Some("HEAD"), "@")?);
                }
                Ok(())
            },
        )?;
    }
    let f = Fixture::new("history")?;
    let mut stream = String::new();
    for index in 0..128 {
        let message = format!("revision {index}\n");
        let contents = format!("const REVISION: usize = {index};\n");
        stream.push_str(&format!("commit refs/heads/main\ncommitter Profile <profile@example.invalid> {} +0000\ndata {}\n{}M 100644 inline history.rs\ndata {}\n{}\n", 1_700_000_000 + index, message.len(), message, contents.len(), contents));
    }
    let mut child = f
        .command("git")
        .args(["fast-import", "--quiet"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("fast-import stdin missing")?
        .write_all(stream.as_bytes())
        .map_err(|e| e.to_string())?;
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err("fast-import failed".into());
    }
    let comparison = Comparison::load(&f.0, Some("HEAD~127"), "HEAD")?;
    measure("git/history/128-commits", args.samples, || {
        comparison.history(&HistoryQuery::default())
    })?;
    measure("git/overview-series/128-commits", args.samples, || {
        comparison.overview_series()
    })?;
    f.run("git", &["remote", "add", "origin", "."])?;
    let head = f.run("git", &["rev-parse", "HEAD"])?;
    let mut previous = 0;
    for count in [700, 1400] {
        let mut updates = String::new();
        for index in previous..(count / 2) {
            updates.push_str(&format!(
                "update refs/remotes/origin/feature-{index:05} {head}\n"
            ));
            updates.push_str(&format!("update refs/heads/feature-{index:05} {head}\n"));
        }
        let mut child = f
            .command("git")
            .args(["update-ref", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .take()
            .ok_or("update-ref stdin missing")?
            .write_all(updates.as_bytes())
            .map_err(|e| e.to_string())?;
        if !child.wait().map_err(|e| e.to_string())?.success() {
            return Err("update-ref failed".into());
        }
        previous = count / 2;
        measure(
            &format!("git/references/{}-entries", count + 1),
            args.samples,
            || comparison.reference_snapshot(),
        )?;
        measure(
            &format!("git/history-with-references/{}-entries", count + 1),
            args.samples,
            || comparison.history(&HistoryQuery::default()),
        )?;
    }
    if args.wide_history {
        let f = Fixture::new("wide-history")?;
        f.write("file.rs", "fn root() {}\n")?;
        f.commit("root")?;
        let root = f.run("git", &["rev-parse", "HEAD"])?;
        let mut stream = String::new();
        for index in 0..700 {
            let message = format!("branch {index}\n");
            let contents = format!("fn branch_{index}() {{}}\n");
            stream.push_str(&format!("commit refs/heads/feature-{index:04}\ncommitter Profile <profile@example.invalid> {} +0000\ndata {}\n{}from {root}\nM 100644 inline file.rs\ndata {}\n{}\n",1_700_000_000+index,message.len(),message,contents.len(),contents));
        }
        let mut child = f
            .command("git")
            .args(["fast-import", "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .take()
            .ok_or("fast-import stdin missing")?
            .write_all(stream.as_bytes())
            .map_err(|e| e.to_string())?;
        if !child.wait().map_err(|e| e.to_string())?.success() {
            return Err("wide history import failed".into());
        }
        let comparison = Comparison::load(&f.0, Some("HEAD"), "HEAD")?;
        let query = HistoryQuery::default().with_limit(1000)?;
        measure("git/history/700-distinct-branch-tips", args.samples, || {
            comparison.history(&query)
        })?;
    }
    if args.jj {
        let f = Fixture::new("jj")?;
        f.write("file.rs", "fn initial() {}\n")?;
        f.commit("baseline")?;
        f.run("jj", &["git", "init", "--colocate"])?;
        for index in 0..32 {
            f.write("file.rs", &format!("fn revision_{index}() {{}}\n"))?;
            f.run("jj", &["describe", "-m", &format!("rewrite {index}")])?;
        }
        measure("jj/open/32-rewrites", args.samples, || {
            Comparison::load(&f.0, None, "@")
        })?;
        let comparison = Comparison::load(&f.0, None, "@")?;
        measure("jj/history/32-rewrites", args.samples, || {
            comparison.history(&HistoryQuery::default())
        })?;
        measure("jj/evolution/32-rewrites", args.samples, || {
            comparison.evolution(comparison.tip(), 100)
        })?;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
