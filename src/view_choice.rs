//! User preference survives temporary file-driven presentation choices.
use crate::settings::DiffView;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilePresentation {
    #[default]
    Comparison,
    SingleFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewChoice {
    preferred: DiffView,
    presentation: FilePresentation,
}
impl ViewChoice {
    pub fn new(preferred: DiffView) -> Self {
        Self {
            preferred,
            presentation: FilePresentation::Comparison,
        }
    }
    pub fn preferred(self) -> DiffView {
        self.preferred
    }
    pub fn effective(self) -> DiffView {
        match self.presentation {
            FilePresentation::Comparison => self.preferred,
            FilePresentation::SingleFile => DiffView::Inline,
        }
    }
    pub fn select(&mut self, preferred: DiffView) {
        self.preferred = preferred;
    }
    pub fn present(&mut self, presentation: FilePresentation) {
        self.presentation = presentation;
    }
}

#[cfg(test)]
mod tests {
    use super::{FilePresentation, ViewChoice};
    use crate::settings::DiffView;
    #[test]
    fn automatic_presentation_does_not_overwrite_the_saved_preference() {
        for preferred in [DiffView::Fluid, DiffView::Inline, DiffView::SideBySide] {
            let mut view = ViewChoice::new(preferred);
            assert_eq!(view.effective(), preferred);
            view.present(FilePresentation::SingleFile);
            assert_eq!(view.effective(), DiffView::Inline);
            assert_eq!(view.preferred(), preferred);
            view.present(FilePresentation::Comparison);
            assert_eq!(view.effective(), preferred);
        }
    }
    #[test]
    fn explicit_choice_during_an_override_is_used_when_normal_viewing_resumes() {
        let mut view = ViewChoice::new(DiffView::Fluid);
        view.present(FilePresentation::SingleFile);
        view.select(DiffView::SideBySide);
        assert_eq!(view.effective(), DiffView::Inline);
        assert_eq!(view.preferred(), DiffView::SideBySide);
        view.present(FilePresentation::Comparison);
        assert_eq!(view.effective(), DiffView::SideBySide);
    }
}
