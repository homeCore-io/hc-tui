//! Selection that follows an item, not a row number.
//!
//! A list keeps the id of the selected item. Every frame the app rebuilds
//! the visible list and calls [`ListSel::resolve`], which finds that id
//! again and updates the row index. Re-sorts, refreshes and live updates
//! therefore keep the cursor on the same item. When the item disappears
//! the cursor stays at the same row (clamped) and adopts whatever item
//! is there now.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListSel {
    id: Option<String>,
    index: usize,
}

impl ListSel {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Re-find the selected id in `ids`, or clamp and adopt the item at
    /// the current row.
    pub fn resolve<S: AsRef<str>>(&mut self, ids: &[S]) {
        if ids.is_empty() {
            self.index = 0;
            return;
        }
        if let Some(id) = &self.id
            && let Some(pos) = ids.iter().position(|x| x.as_ref() == id)
        {
            self.index = pos;
            return;
        }
        self.index = self.index.min(ids.len() - 1);
        self.id = Some(ids[self.index].as_ref().to_string());
    }

    /// Move by `delta` rows within `ids`, clamped at both ends.
    pub fn move_by<S: AsRef<str>>(&mut self, delta: isize, ids: &[S]) {
        if ids.is_empty() {
            self.index = 0;
            return;
        }
        // Start from where the id is now, in case the list moved since
        // the last resolve.
        self.resolve(ids);
        let max = ids.len() as isize - 1;
        let next = (self.index as isize + delta).clamp(0, max) as usize;
        self.index = next;
        self.id = Some(ids[next].as_ref().to_string());
    }

    /// Jump to row `index` (clamped).
    #[cfg(test)]
    pub fn select_index<S: AsRef<str>>(&mut self, index: usize, ids: &[S]) {
        if ids.is_empty() {
            self.index = 0;
            self.id = None;
            return;
        }
        self.index = index.min(ids.len() - 1);
        self.id = Some(ids[self.index].as_ref().to_string());
    }

    /// Select the item with `id`, wherever it is.
    pub fn select_id(&mut self, id: impl Into<String>) {
        self.id = Some(id.into());
    }

    /// Forget the item and go back to the first row.
    pub fn reset(&mut self) {
        self.id = None;
        self.index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_follows_the_item_after_a_resort() {
        let mut sel = ListSel::default();
        let ids = vec!["a", "b", "c", "d"];
        sel.resolve(&ids);
        sel.move_by(2, &ids);
        assert_eq!(sel.id(), Some("c"));
        assert_eq!(sel.index(), 2);

        // Re-sorted: c is now first.
        let resorted = vec!["c", "a", "d", "b"];
        sel.resolve(&resorted);
        assert_eq!(sel.id(), Some("c"));
        assert_eq!(sel.index(), 0);
    }

    #[test]
    fn removed_item_keeps_the_row() {
        let mut sel = ListSel::default();
        let ids = vec!["a", "b", "c"];
        sel.select_index(2, &ids);
        let shorter = vec!["a", "b"];
        sel.resolve(&shorter);
        assert_eq!(sel.index(), 1);
        assert_eq!(sel.id(), Some("b"));
    }

    #[test]
    fn moves_clamp() {
        let mut sel = ListSel::default();
        let ids = vec!["a", "b"];
        sel.move_by(-5, &ids);
        assert_eq!(sel.index(), 0);
        sel.move_by(5, &ids);
        assert_eq!(sel.index(), 1);
        let empty: Vec<&str> = vec![];
        sel.move_by(1, &empty);
        assert_eq!(sel.index(), 0);
    }

    #[test]
    fn empty_list_keeps_the_id_for_later() {
        let mut sel = ListSel::default();
        sel.select_id("x");
        let empty: Vec<&str> = vec![];
        sel.resolve(&empty);
        let back = vec!["y", "x"];
        sel.resolve(&back);
        assert_eq!(sel.index(), 1);
    }
}
