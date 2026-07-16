use std::path::{Path, PathBuf};

const DEFAULT_HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug)]
pub struct NavigationHistory {
    back: Vec<PathBuf>,
    current: Option<PathBuf>,
    forward: Vec<PathBuf>,
    limit: usize,
}

impl Default for NavigationHistory {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            current: None,
            forward: Vec::new(),
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }
}

impl NavigationHistory {
    pub fn restore(&mut self, path: PathBuf) {
        self.back.clear();
        self.forward.clear();
        self.current = Some(path);
    }

    pub fn visit(&mut self, path: PathBuf) -> bool {
        if self.current.as_ref() == Some(&path) {
            return false;
        }
        if let Some(current) = self.current.replace(path) {
            self.back.push(current);
            trim_front(&mut self.back, self.limit);
        }
        self.forward.clear();
        true
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        let destination = self.back.pop()?;
        if let Some(current) = self.current.replace(destination.clone()) {
            self.forward.push(current);
            trim_front(&mut self.forward, self.limit);
        }
        Some(destination)
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        let destination = self.forward.pop()?;
        if let Some(current) = self.current.replace(destination.clone()) {
            self.back.push(current);
            trim_front(&mut self.back, self.limit);
        }
        Some(destination)
    }

    pub fn up(&self) -> Option<PathBuf> {
        self.current
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn can_up(&self) -> bool {
        self.up().is_some()
    }
}

fn trim_front(history: &mut Vec<PathBuf>, limit: usize) {
    let overflow = history.len().saturating_sub(limit);
    if overflow > 0 {
        history.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::NavigationHistory;
    use std::path::PathBuf;

    #[test]
    fn back_and_forward_preserve_browser_history() {
        let mut history = NavigationHistory::default();
        history.restore(PathBuf::from(r"C:\Products"));
        history.visit(PathBuf::from(r"C:\Products\100"));
        history.visit(PathBuf::from(r"C:\Products\200"));

        assert_eq!(history.back(), Some(PathBuf::from(r"C:\Products\100")));
        assert_eq!(history.back(), Some(PathBuf::from(r"C:\Products")));
        assert!(!history.can_back());
        assert_eq!(history.forward(), Some(PathBuf::from(r"C:\Products\100")));
        assert!(history.can_forward());
    }

    #[test]
    fn visiting_after_back_clears_forward_history() {
        let mut history = NavigationHistory::default();
        history.restore(PathBuf::from(r"C:\Products"));
        history.visit(PathBuf::from(r"C:\Products\100"));
        history.back();
        history.visit(PathBuf::from(r"C:\Products\300"));

        assert!(!history.can_forward());
        assert_eq!(history.up(), Some(PathBuf::from(r"C:\Products")));
    }
}
