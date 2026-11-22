//! Undo and redo commands.

use super::*;

impl App {
    /// `u`: undo.
    pub(super) fn undo_step(&mut self) {
        let link = self.link_ids();
        match self.undo.undo(&mut self.doc) {
            Some(label) => {
                self.rev += 1;
                self.after_history(&format!("撤销:{label}"));
                self.rebind_link(link);
            }
            None => {
                let note = self
                    .view
                    .lang
                    .pick_owned("没有可撤销的".to_string(), "nothing to undo".to_string());
                self.view.note = Some(note);
            }
        }
    }

    /// `Ctrl-r`: redo.
    pub(super) fn redo_step(&mut self) {
        let link = self.link_ids();
        match self.undo.redo(&mut self.doc) {
            Some(label) => {
                self.rev += 1;
                self.after_history(&format!("重做:{label}"));
                self.rebind_link(link);
            }
            None => {
                let note = self
                    .view
                    .lang
                    .pick_owned("没有可重做的".to_string(), "nothing to redo".to_string());
                self.view.note = Some(note);
            }
        }
    }

    /// After undo or redo: clamp the cursor into the document, clear selection and
    /// edge selection.
    pub(super) fn after_history(&mut self, note: &str) {
        self.view.sel = None;
        self.view.cand = 0;
        if self.doc.boxes.is_empty() {
            self.view.cursor = (0, 0);
        }
        self.refresh_pick();
        self.view.note = Some(note.to_string());
    }
}
