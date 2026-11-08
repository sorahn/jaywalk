use clap::Parser;
use jaywalk::repository::Comparison;
use std::path::{Path, PathBuf};

#[derive(Clone, Parser, serde::Serialize, serde::Deserialize)]
#[command(about = "Jaywalk: a native Jujutsu diff viewer")]
pub(crate) struct Arguments {
    #[arg(default_value = ".")]
    pub(crate) directory: PathBuf,
    #[arg(long, conflicts_with = "diff")]
    pub(crate) base: Option<String>,
    #[arg(long, default_value = "@", conflicts_with = "diff")]
    pub(crate) tip: String,
    /// Compare two files or directory trees, without requiring a repository.
    #[arg(long, num_args = 2, value_names = ["LEFT", "RIGHT"])]
    pub(crate) diff: Option<Vec<PathBuf>>,
    /// Wait until the launched window closes (required for temporary diff inputs).
    #[arg(long)]
    pub(crate) wait: bool,
    #[arg(long)]
    pub(crate) inspect: bool,
    /// Keep the GUI attached to this terminal (useful for debugging).
    #[arg(long)]
    pub(crate) foreground: bool,
    // A bundled executable reached through jw still needs normal CLI detachment.
    #[arg(long, hide = true)]
    pub(crate) terminal_launch: bool,
    #[arg(long, hide = true)]
    pub(crate) instance_server: bool,
    #[arg(long)]
    pub(crate) file: Option<String>,
    /// Read settings from this TOML file instead of the default config path.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
}

impl Arguments {
    pub(crate) fn comparison(&self) -> std::result::Result<Comparison, String> {
        match &self.diff {
            Some(paths) => jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]),
            None => Comparison::load(&self.directory, self.base.as_deref(), &self.tip),
        }
    }
}

impl Arguments {
    /// Resolve client cwd and aliases before crossing the process boundary.
    pub(crate) fn normalize(&mut self) -> Result<(), String> {
        let canonical = |path: &Path| {
            std::fs::canonicalize(path)
                .map_err(|error| format!("Could not open {}: {error}", path.display()))
        };
        self.directory = canonical(&self.directory)?;
        if self.diff.is_none()
            && let Some(root) = self
                .directory
                .ancestors()
                .find(|path| path.join(".jj").is_dir() || path.join(".git").exists())
        {
            self.directory = root.to_owned();
        }
        if let Some(paths) = &mut self.diff {
            if paths.len() != 2 {
                return Err("A diff launch requires two input paths.".into());
            }
            for path in paths {
                *path = canonical(path)?;
            }
        }
        if let Some(config) = &mut self.config {
            *config = canonical(config)?;
        }
        Ok(())
    }
}
