use clap::Parser;
use jaywalk::repository::Comparison;
use std::path::{Path, PathBuf};

#[derive(Clone, Parser, serde::Serialize, serde::Deserialize)]
#[command(about = "Jaywalk: a native Git and Jujutsu diff viewer", version = jaywalk::version::DISPLAY)]
pub(crate) struct Arguments {
    /// Repository directory. Omit it to open the welcome page; use . for this directory.
    #[arg(value_name = "DIRECTORY")]
    pub(crate) directory: Option<PathBuf>,
    // Internal launch intent survives normalization and single-instance forwarding.
    #[arg(skip)]
    #[serde(default)]
    pub(crate) welcome: bool,
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
    /// Machine-readable release version for the bundle builder.
    #[arg(long, hide = true)]
    #[serde(default)]
    pub(crate) package_version: bool,
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
    pub(crate) fn directory(&self) -> &Path {
        self.directory.as_deref().unwrap_or(Path::new("."))
    }
    pub(crate) fn comparison(&self) -> std::result::Result<Comparison, String> {
        match &self.diff {
            Some(paths) => jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]),
            None => Comparison::load(self.directory(), self.base.as_deref(), &self.tip),
        }
    }
}

impl Arguments {
    /// Resolve client cwd and aliases before crossing the process boundary.
    pub(crate) fn normalize(&mut self) -> Result<(), String> {
        let directory = std::env::current_dir()
            .map_err(|error| format!("Could not resolve the launch directory: {error}"))?;
        self.normalize_at(&directory)
    }
    fn normalize_at(&mut self, directory: &Path) -> Result<(), String> {
        if self.directory.is_none()
            && self.diff.is_none()
            && self.base.is_none()
            && self.tip == "@"
            && self.file.is_none()
            && !self.inspect
        {
            self.welcome = true;
        }
        let canonical = |path: &Path| {
            let resolved = if path.is_absolute() {
                path.to_owned()
            } else {
                directory.join(path)
            };
            std::fs::canonicalize(&resolved)
                .map_err(|error| format!("Could not open {}: {error}", resolved.display()))
        };
        self.directory = Some(canonical(self.directory())?);
        if self.diff.is_none() && !self.welcome {
            if let Some(root) = self
                .directory()
                .ancestors()
                .find(|path| path.join(".jj").is_dir() || path.join(".git").exists())
            {
                self.directory = Some(root.to_owned());
            } else if self.base.is_none() && self.tip == "@" && self.file.is_none() {
                // A bare launch outside a repository is a normal welcome state.
                self.welcome = true;
            }
        }
        if let Some(paths) = &mut self.diff {
            if paths.len() != 2 {
                return Err("A diff launch requires two input paths.".into());
            }
            for path in paths {
                let input = if path.is_absolute() {
                    path.clone()
                } else {
                    directory.join(&*path)
                };
                *path = jaywalk::external::normalize_input(&input)?;
            }
        }
        if let Some(config) = &mut self.config {
            *config = canonical(config)?;
        }
        Ok(())
    }
}

impl Arguments {
    pub(crate) fn for_repository(&self, directory: PathBuf) -> Self {
        let mut arguments = self.clone();
        arguments.directory = Some(directory);
        arguments.welcome = false;
        arguments.diff = None;
        arguments.base = None;
        arguments.tip = "@".into();
        arguments.file = None;
        arguments.wait = false;
        arguments
    }
    pub(crate) fn for_welcome(&self) -> Self {
        let mut arguments = self.for_repository(self.directory().to_owned());
        arguments.welcome = true;
        arguments
    }
}

#[cfg(test)]
mod welcome_tests {
    use super::Arguments;
    use clap::Parser;
    #[test]
    fn bare_launch_stays_welcome_inside_a_repository_and_dot_is_explicit() {
        let root = std::env::temp_dir().join(format!("jw-bare-launch-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".jj")).unwrap();
        let mut bare = Arguments::parse_from(["jaywalk"]);
        bare.normalize_at(&root).unwrap();
        assert!(bare.welcome);
        let serialized = serde_json::to_vec(&bare).unwrap();
        let mut forwarded: Arguments = serde_json::from_slice(&serialized).unwrap();
        forwarded.normalize_at(&std::env::temp_dir()).unwrap();
        assert!(forwarded.welcome);
        let mut explicit = Arguments::parse_from(["jaywalk", "."]);
        explicit.normalize_at(&root).unwrap();
        assert!(!explicit.welcome);
        assert_eq!(explicit.directory(), std::fs::canonicalize(&root).unwrap());
        assert!(explicit.for_welcome().welcome);
        let mut target = Arguments::parse_from(["jaywalk", "--base", "main"]);
        target.normalize_at(&root).unwrap();
        assert!(!target.welcome);
        std::fs::write(root.join("left"), "before").unwrap();
        std::fs::write(root.join("right"), "after").unwrap();
        let mut diff = Arguments::parse_from(["jaywalk", "--diff", "left", "right"]);
        diff.normalize_at(&root).unwrap();
        assert!(!diff.welcome);
        assert_eq!(
            diff.diff.as_ref().unwrap()[0],
            std::fs::canonicalize(root.join("left")).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn repository_free_launches_show_welcome_but_explicit_comparisons_keep_their_errors() {
        let root = std::env::temp_dir().join(format!("jw-welcome-cli-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut plain = Arguments::parse_from([std::ffi::OsStr::new("jaywalk"), root.as_os_str()]);
        plain.normalize().unwrap();
        assert!(plain.welcome);
        let mut explicit = plain.for_repository(root.clone());
        explicit.base = Some("main".into());
        explicit.normalize().unwrap();
        assert!(!explicit.welcome);
        assert!(explicit.comparison().is_err());
        std::fs::create_dir_all(root.join("repo/.jj")).unwrap();
        std::fs::create_dir_all(root.join("repo/src")).unwrap();
        let mut repo = plain.for_repository(root.join("repo/src"));
        repo.normalize().unwrap();
        assert!(!repo.welcome);
        assert_eq!(
            repo.directory(),
            std::fs::canonicalize(root.join("repo")).unwrap()
        );
        assert!(repo.for_welcome().welcome);
        assert!(repo.for_welcome().base.is_none());
        assert!(Arguments::try_parse_from(["jaywalk", "--welcome"]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
