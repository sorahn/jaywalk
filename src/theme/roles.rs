//! Canonical roles follow panel identity, never physical placement.
use serde::Deserialize;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Window,
    Header,
    Footer,
    History,
    Files,
    HistoryHeader,
    FilesHeader,
    HistoryFooter,
    FilesFooter,
    Viewer,
    ViewerHeader,
    Floating,
}
impl Surface {
    pub const ALL: [Self; 12] = [
        Self::Window,
        Self::Header,
        Self::Footer,
        Self::History,
        Self::Files,
        Self::HistoryHeader,
        Self::FilesHeader,
        Self::HistoryFooter,
        Self::FilesFooter,
        Self::Viewer,
        Self::ViewerHeader,
        Self::Floating,
    ];
    pub const fn path(self) -> &'static str {
        match self {
            Self::Window => "shell.window",
            Self::Header => "shell.header",
            Self::Footer => "shell.footer",
            Self::History => "shell.sidebar.history",
            Self::Files => "shell.sidebar.files",
            Self::HistoryHeader => "shell.sidebar.history.header",
            Self::FilesHeader => "shell.sidebar.files.header",
            Self::HistoryFooter => "shell.sidebar.history.footer",
            Self::FilesFooter => "shell.sidebar.files.footer",
            Self::Viewer => "editor",
            Self::ViewerHeader => "editor.header",
            Self::Floating => "shell.floating",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Background,
    Foreground,
    MutedForeground,
    Border,
}
impl Part {
    pub const ALL: [Self; 4] = [
        Self::Background,
        Self::Foreground,
        Self::MutedForeground,
        Self::Border,
    ];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub enum Role {
    Surface(Surface, Part),
    Backdrop,
    GraphLane(u8),
    EvolutionCurrent,
    EvolutionMuted,
    Success,
    Warning,
    Error,
    Info,
    Renamed,
    Copied,
    ControlBackground,
    ControlHover,
    ControlForeground,
}
impl Role {
    pub fn all() -> Vec<Self> {
        let mut roles: Vec<_> = Surface::ALL
            .into_iter()
            .flat_map(|surface| Part::ALL.map(|part| Self::Surface(surface, part)))
            .collect();
        roles.extend((0..5).map(Self::GraphLane));
        roles.extend([
            Self::Backdrop,
            Self::EvolutionCurrent,
            Self::EvolutionMuted,
            Self::Success,
            Self::Warning,
            Self::Error,
            Self::Info,
            Self::Renamed,
            Self::Copied,
            Self::ControlBackground,
            Self::ControlHover,
            Self::ControlForeground,
        ]);
        roles
    }
}
impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Surface(Surface::Window, Part::Foreground) => f.write_str("shell.foreground"),
            Self::Surface(Surface::Window, Part::MutedForeground) => {
                f.write_str("shell.foreground.muted")
            }
            Self::Surface(Surface::Window, Part::Border) => f.write_str("shell.border"),
            Self::Surface(surface, part) => write!(
                f,
                "{}.{}",
                surface.path(),
                match part {
                    Part::Background => "background",
                    Part::Foreground => "foreground",
                    Part::MutedForeground => "foreground.muted",
                    Part::Border => "border",
                }
            ),
            Self::GraphLane(lane) => write!(f, "graph.lanes.{lane}"),
            other => f.write_str(match other {
                Self::Backdrop => "shell.backdrop",
                Self::EvolutionCurrent => "graph.evolution.current",
                Self::EvolutionMuted => "graph.evolution.muted",
                Self::Success => "status.success",
                Self::Warning => "status.warning",
                Self::Error => "status.error",
                Self::Info => "status.info",
                Self::Renamed => "diff.renamed.marker",
                Self::Copied => "diff.copied.marker",
                Self::ControlBackground => "shell.control.background",
                Self::ControlHover => "shell.control.hover",
                Self::ControlForeground => "shell.control.foreground",
                _ => unreachable!("surface and graph roles handled above"),
            }),
        }
    }
}
impl TryFrom<String> for Role {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        Self::all()
            .into_iter()
            .find(|r| r.to_string() == value)
            .ok_or_else(|| format!("unknown theme color role {value:?}"))
    }
}
