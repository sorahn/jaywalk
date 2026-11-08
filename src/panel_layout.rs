//! Panel identity and placement are independent of widget instances and content.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    History,
    Files,
    ChangeEvolution,
    FileEvolution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarOrder {
    HistoryFirst,
    FilesFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionDock {
    Sidebar,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub history_side: Side,
    pub files_side: Side,
    /// Left-to-right order when both panels share a side.
    pub sidebar_order: SidebarOrder,
    pub change_evolution: EvolutionDock,
    pub file_evolution: EvolutionDock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    History,
    Files,
    Workspace,
}

impl Placement {
    /// Only visible panels participate in resize slots; a hidden panel must
    /// never displace another panel or leave a phantom slot beside the workspace.
    pub fn columns(&self, history_visible: bool, files_visible: bool) -> Vec<Column> {
        let panel = |id| match id {
            PanelId::History => Column::History,
            PanelId::Files => Column::Files,
            _ => unreachable!("sidebars only contain History and Files"),
        };
        self.sidebars(Side::Left)
            .into_iter()
            .map(panel)
            .chain(std::iter::once(Column::Workspace))
            .chain(self.sidebars(Side::Right).into_iter().map(panel))
            .filter(|column| match column {
                Column::History => history_visible,
                Column::Files => files_visible,
                Column::Workspace => true,
            })
            .collect()
    }

    pub fn sidebars(&self, side: Side) -> Vec<PanelId> {
        let ordered = match self.sidebar_order {
            SidebarOrder::HistoryFirst => [PanelId::History, PanelId::Files],
            SidebarOrder::FilesFirst => [PanelId::Files, PanelId::History],
        };
        ordered
            .into_iter()
            .filter(|panel| match panel {
                PanelId::History => self.history_side == side,
                PanelId::Files => self.files_side == side,
                _ => false,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hiding_one_right_sidebar_keeps_the_other_and_the_workspace() {
        let placement = Placement {
            history_side: Side::Right,
            files_side: Side::Right,
            sidebar_order: SidebarOrder::FilesFirst,
            change_evolution: EvolutionDock::Sidebar,
            file_evolution: EvolutionDock::Bottom,
        };
        assert_eq!(
            placement.columns(false, true),
            [Column::Workspace, Column::Files]
        );
        assert_eq!(
            placement.columns(true, false),
            [Column::Workspace, Column::History]
        );
        assert_eq!(placement.columns(false, false), [Column::Workspace]);
        assert_eq!(
            placement.columns(true, true),
            [Column::Workspace, Column::Files, Column::History]
        );
    }

    #[test]
    fn every_arrangement_keeps_each_sidebar_once_on_its_own_side() {
        for history_side in [Side::Left, Side::Right] {
            for files_side in [Side::Left, Side::Right] {
                for sidebar_order in [SidebarOrder::HistoryFirst, SidebarOrder::FilesFirst] {
                    let placement = Placement {
                        history_side,
                        files_side,
                        sidebar_order,
                        change_evolution: EvolutionDock::Sidebar,
                        file_evolution: EvolutionDock::Bottom,
                    };
                    let all: Vec<_> = [Side::Left, Side::Right]
                        .into_iter()
                        .flat_map(|side| placement.sidebars(side))
                        .collect();
                    assert_eq!(all.len(), 2);
                    assert_eq!(all.iter().filter(|&&id| id == PanelId::History).count(), 1);
                    assert_eq!(all.iter().filter(|&&id| id == PanelId::Files).count(), 1);
                    if history_side == files_side {
                        assert_eq!(
                            all[0],
                            match sidebar_order {
                                SidebarOrder::HistoryFirst => PanelId::History,
                                SidebarOrder::FilesFirst => PanelId::Files,
                            }
                        );
                    }
                }
            }
        }
    }
}
