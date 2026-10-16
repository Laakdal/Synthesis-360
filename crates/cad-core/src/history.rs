//! Undo/redo as document snapshots.
//!
//! Every committed operation stores the complete document state before it. Undo restores the
//! previous snapshot and the model is recomputed from it, so undo is exact for parameters,
//! sketches, the timeline marker and suppression alike (observed in Fusion: Undo removed the
//! last timeline item, 8 -> 7, and Redo restored it).

use crate::document::Document;

#[derive(Clone, Debug)]
struct Entry {
    label: String,
    doc: Document,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    limit: usize,
}

impl History {
    pub fn new() -> Self {
        History { undo: Vec::new(), redo: Vec::new(), limit: 200 }
    }

    /// Records `before` as the state to return to when the operation `label` is undone.
    pub fn record(&mut self, label: &str, before: &Document) {
        self.undo.push(Entry { label: label.into(), doc: before.clone() });
        if self.undo.len() > self.limit.max(1) {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Returns the document to restore; `current` becomes redoable.
    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let e = self.undo.pop()?;
        self.redo.push(Entry { label: e.label.clone(), doc: current.clone() });
        Some(e.doc)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let e = self.redo.pop()?;
        self.undo.push(Entry { label: e.label.clone(), doc: current.clone() });
        Some(e.doc)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Quantity;

    #[test]
    fn undo_redo_round_trip() {
        let mut h = History::new();
        let mut doc = Document::new("t");
        h.record("Add d1", &doc);
        doc.add_model_parameter("10 mm", Quantity::Length, "f");
        h.record("Edit d1", &doc);
        doc.set_expression("d1", "20 mm").unwrap();
        assert_eq!(h.undo_label(), Some("Edit d1"));
        doc = h.undo(&doc).unwrap();
        assert_eq!(doc.parameter("d1").unwrap().expression, "10 mm");
        doc = h.undo(&doc).unwrap();
        assert!(doc.parameters.is_empty());
        assert!(!h.can_undo());
        doc = h.redo(&doc).unwrap();
        doc = h.redo(&doc).unwrap();
        assert_eq!(doc.parameter("d1").unwrap().expression, "20 mm");
        // a new operation clears the redo stack
        h.undo(&doc);
        h.record("Other", &doc);
        assert!(!h.can_redo());
    }
}
