//! The aggregate being reviewed is separate from a selected commit or file.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HomeSource {
    Workspace,
    Opened,
}
impl HomeSource {
    pub(super) fn from_arguments(arguments: &Arguments) -> Self {
        if arguments.diff.is_some() || arguments.base.is_some() || arguments.tip != "@" {
            Self::Opened
        } else {
            Self::Workspace
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Current work",
            Self::Opened => "Opened comparison",
        }
    }
}
#[derive(Clone)]
pub(super) struct ReviewContext {
    pub(super) source: HomeSource,
    pub(super) aggregate: Rc<Comparison>,
}
impl Viewer {
    pub(super) fn home_arguments(&self) -> Arguments {
        let mut arguments = self.arguments.clone();
        if self.home_source == HomeSource::Workspace {
            arguments.base = None;
            arguments.tip = "@".into();
            arguments.diff = None;
        }
        arguments
    }
}

#[cfg(test)]
mod tests {
    use super::HomeSource;
    use crate::cli::Arguments;
    use clap::Parser;
    #[test]
    fn explicit_ranges_and_external_inputs_remain_distinct_from_current_work() {
        for arguments in [
            vec!["jw", "."],
            vec!["jw", ".", "--base", "main", "--tip", "feature"],
            vec!["jw", "--diff", "before", "after"],
        ] {
            let parsed = Arguments::try_parse_from(&arguments).unwrap();
            assert_eq!(
                HomeSource::from_arguments(&parsed),
                if arguments.len() == 2 {
                    HomeSource::Workspace
                } else {
                    HomeSource::Opened
                }
            );
        }
    }
}
