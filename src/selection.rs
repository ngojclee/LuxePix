use std::collections::BTreeSet;

use crate::model::ItemId;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionModel {
    selected: BTreeSet<ItemId>,
    anchor: Option<ItemId>,
    focus: Option<ItemId>,
}

impl SelectionModel {
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn contains(&self, id: &ItemId) -> bool {
        self.selected.contains(id)
    }

    pub fn focus(&self) -> Option<&ItemId> {
        self.focus
            .as_ref()
            .filter(|focus| self.selected.contains(*focus))
            .or_else(|| self.selected.iter().next())
    }

    pub fn single(&self) -> Option<&ItemId> {
        (self.selected.len() == 1)
            .then(|| self.selected.iter().next())
            .flatten()
    }

    pub fn ids(&self) -> impl Iterator<Item = &ItemId> {
        self.selected.iter()
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    pub fn replace(&mut self, id: ItemId) {
        self.selected.clear();
        self.selected.insert(id.clone());
        self.anchor = Some(id.clone());
        self.focus = Some(id);
    }

    pub fn apply_click(&mut self, id: ItemId, ordered: &[ItemId], ctrl: bool, shift: bool) {
        if shift {
            self.select_range(id, ordered, ctrl);
        } else if ctrl {
            self.toggle(id);
        } else {
            self.replace(id);
        }
    }

    pub fn select_all(&mut self, ordered: &[ItemId]) {
        self.selected = ordered.iter().cloned().collect();
        if self.focus().is_none() {
            self.focus = ordered.first().cloned();
        }
        if self.anchor.is_none() {
            self.anchor = self.focus.clone();
        }
    }

    pub fn move_focus(&mut self, ordered: &[ItemId], delta: isize, extend: bool) -> Option<ItemId> {
        if ordered.is_empty() {
            self.clear();
            return None;
        }

        let current = self
            .focus()
            .and_then(|focus| ordered.iter().position(|id| id == focus))
            .unwrap_or_else(|| if delta < 0 { ordered.len() - 1 } else { 0 });
        let destination = current
            .saturating_add_signed(delta)
            .min(ordered.len().saturating_sub(1));
        let id = ordered[destination].clone();
        if extend {
            self.select_range(id.clone(), ordered, false);
        } else {
            self.replace(id.clone());
        }
        Some(id)
    }

    pub fn retain(&mut self, ordered: &[ItemId]) {
        let available: BTreeSet<_> = ordered.iter().cloned().collect();
        self.selected.retain(|id| available.contains(id));
        if self
            .focus
            .as_ref()
            .is_some_and(|focus| !self.selected.contains(focus))
        {
            self.focus = self.selected.iter().next().cloned();
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|anchor| !available.contains(anchor))
        {
            self.anchor = self.focus.clone();
        }
        if self.selected.is_empty() {
            self.focus = None;
            self.anchor = None;
        }
    }

    pub fn add_many<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = ItemId>,
    {
        for id in ids {
            self.focus = Some(id.clone());
            self.selected.insert(id);
        }
        if self.anchor.is_none() {
            self.anchor = self.focus.clone();
        }
    }

    pub fn replace_id(&mut self, old: &ItemId, new: ItemId) {
        if self.selected.remove(old) {
            self.selected.insert(new.clone());
        }
        if self.anchor.as_ref() == Some(old) {
            self.anchor = Some(new.clone());
        }
        if self.focus.as_ref() == Some(old) {
            self.focus = Some(new);
        }
    }

    fn toggle(&mut self, id: ItemId) {
        if !self.selected.remove(&id) {
            self.selected.insert(id.clone());
        }
        self.anchor = Some(id.clone());
        self.focus = if self.selected.contains(&id) {
            Some(id)
        } else {
            self.selected.iter().next_back().cloned()
        };
        if self.selected.is_empty() {
            self.anchor = None;
        }
    }

    fn select_range(&mut self, id: ItemId, ordered: &[ItemId], additive: bool) {
        let anchor = self
            .anchor
            .as_ref()
            .or_else(|| self.focus())
            .cloned()
            .unwrap_or_else(|| id.clone());
        let Some(anchor_index) = ordered.iter().position(|candidate| candidate == &anchor) else {
            self.replace(id);
            return;
        };
        let Some(target_index) = ordered.iter().position(|candidate| candidate == &id) else {
            return;
        };

        if !additive {
            self.selected.clear();
        }
        let start = anchor_index.min(target_index);
        let end = anchor_index.max(target_index);
        self.selected.extend(ordered[start..=end].iter().cloned());
        self.anchor = Some(anchor);
        self.focus = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionModel;
    use crate::model::ItemId;
    use std::path::PathBuf;

    fn ids() -> Vec<ItemId> {
        (1..=5)
            .map(|value| ItemId::from(PathBuf::from(format!(r"C:\Products\{value}"))))
            .collect()
    }

    #[test]
    fn ctrl_click_toggles_without_losing_other_items() {
        let ordered = ids();
        let mut selection = SelectionModel::default();
        selection.apply_click(ordered[0].clone(), &ordered, false, false);
        selection.apply_click(ordered[2].clone(), &ordered, true, false);

        assert_eq!(selection.len(), 2);
        assert!(selection.contains(&ordered[0]));
        assert!(selection.contains(&ordered[2]));

        selection.apply_click(ordered[0].clone(), &ordered, true, false);
        assert_eq!(selection.len(), 1);
        assert!(!selection.contains(&ordered[0]));
    }

    #[test]
    fn shift_click_selects_an_inclusive_range() {
        let ordered = ids();
        let mut selection = SelectionModel::default();
        selection.replace(ordered[1].clone());
        selection.apply_click(ordered[4].clone(), &ordered, false, true);

        assert_eq!(selection.len(), 4);
        assert!(!selection.contains(&ordered[0]));
        assert!(selection.contains(&ordered[4]));
    }

    #[test]
    fn retain_preserves_only_items_still_present_after_refresh() {
        let ordered = ids();
        let mut selection = SelectionModel::default();
        selection.select_all(&ordered);
        selection.retain(&ordered[2..]);

        assert_eq!(selection.len(), 3);
        assert_eq!(selection.focus(), Some(&ordered[2]));
    }

    #[test]
    fn keyboard_extension_uses_the_existing_anchor() {
        let ordered = ids();
        let mut selection = SelectionModel::default();
        selection.replace(ordered[1].clone());
        selection.move_focus(&ordered, 2, true);

        assert_eq!(selection.len(), 3);
        assert!(selection.contains(&ordered[1]));
        assert!(selection.contains(&ordered[3]));
    }
}
