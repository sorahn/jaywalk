//! Read-only real-repository metadata profiling. Output contains counts, not source or messages.
use clap::Parser;
use jaywalk::{
    accounts::Control,
    history::{History, HistoryQuery, HistorySource, SearchQuery},
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long, default_value = "repository")]
    label: String,
    #[arg(long, default_value_t = jaywalk::history::DEFAULT_HISTORY_LIMIT)]
    page_size: usize,
    #[arg(long, default_value_t = 3)]
    pages: usize,
    #[arg(long)]
    retain_snapshots: bool,
    #[arg(long)]
    initial_batch_size: Option<usize>,
    #[arg(long, default_value = "fix")]
    search: String,
}
fn memory() -> serde_json::Value {
    // SAFETY: getrusage fills a valid zero-initialized rusage structure.
    let (parent, child) = unsafe {
        let mut parent = std::mem::zeroed::<libc::rusage>();
        let mut child = std::mem::zeroed::<libc::rusage>();
        libc::getrusage(libc::RUSAGE_SELF, &mut parent);
        libc::getrusage(libc::RUSAGE_CHILDREN, &mut child);
        (parent.ru_maxrss, child.ru_maxrss)
    };
    let divisor = if cfg!(target_os = "macos") { 1024. } else { 1. };
    serde_json::json!({"parent_peak_kib":parent as f64/divisor,"largest_child_peak_kib":child as f64/divisor})
}
fn report(label: &str, metric: &str, start: Instant, history: Option<&History>) {
    let mut row = serde_json::json!({"label":label,"metric":metric,"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"memory":memory()});
    if let Some(history) = history {
        row["rows"] = history.revisions.len().into();
        row["has_more"] = history.has_more().into();
        row["graph_strokes"] = history
            .graph
            .iter()
            .map(|row| row.strokes.len())
            .sum::<usize>()
            .into();
        row["metadata_text_bytes"] = history
            .revisions
            .iter()
            .map(|r| {
                r.commit_id.len()
                    + r.change_id.len()
                    + r.description.len()
                    + r.author.len()
                    + r.timestamp.len()
                    + r.parents.iter().map(String::len).sum::<usize>()
            })
            .sum::<usize>()
            .into();
    }
    println!("{row}");
}
fn main() -> Result<(), String> {
    let args = Args::parse();
    let query = HistoryQuery::default().with_limit(args.page_size)?;
    let start = Instant::now();
    let source = Arc::new(HistorySource::git(&args.repo, &query)?);
    report(&args.label, "capture", start, None);
    let control = || Control::new(Duration::from_secs(60));
    let start = Instant::now();
    let initial_size = args
        .initial_batch_size
        .unwrap_or(args.page_size)
        .min(args.page_size);
    let mut history = source.first_page(initial_size, &control())?;
    report(&args.label, "initial-page", start, Some(&history));
    if history.has_more() && history.revisions.len() < args.page_size {
        let start = Instant::now();
        history = history.load_more(args.page_size - history.revisions.len(), &control())?;
        report(&args.label, "fill-initial-page", start, Some(&history));
    }
    let mut retained = Vec::new();
    for _ in 1..args.pages {
        if !history.has_more() {
            break;
        }
        let start = Instant::now();
        let next = history.load_more(args.page_size, &control())?;
        if args.retain_snapshots {
            retained.push(history);
        }
        history = next;
        report(&args.label, "append-page", start, Some(&history));
    }
    println!(
        "{}",
        serde_json::json!({"label":args.label,"retained_snapshots":retained.len(),"memory":memory()})
    );
    let start = Instant::now();
    let page = source.search(&SearchQuery::parse(&args.search)?, 0, 100, &control())?;
    report(&args.label, "search", start, None);
    println!(
        "{}",
        serde_json::json!({"label":args.label,"search_matches":page.matches.len(),"scanned_rows":page.scanned,"more_matches":page.next_offset.is_some()})
    );
    Ok(())
}
