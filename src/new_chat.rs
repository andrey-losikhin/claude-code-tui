use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;

#[derive(Debug)]
pub struct DirectoryBrowser {
    pub directory: PathBuf,
    pub children: Vec<PathBuf>,
    pub selected: usize,
}

impl DirectoryBrowser {
    fn read(path: &Path) -> io::Result<Self> {
        let directory = std::fs::canonicalize(path)?;
        let mut children = Vec::new();
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                children.push(path);
            }
        }
        children.sort();
        Ok(Self {
            directory,
            children,
            selected: 0,
        })
    }

    pub fn has_parent(&self) -> bool {
        self.directory.parent().is_some()
    }

    fn row_count(&self) -> usize {
        1 + usize::from(self.has_parent()) + self.children.len()
    }

    fn selected_child(&self) -> Option<&Path> {
        self.selected
            .checked_sub(1 + usize::from(self.has_parent()))
            .and_then(|index| self.children.get(index))
            .map(PathBuf::as_path)
    }
}

#[derive(Debug)]
pub struct NewChatDialog {
    pub projects: Vec<PathBuf>,
    pub selected: usize,
    pub browser: Option<DirectoryBrowser>,
    pub error: Option<String>,
    initial_directory: PathBuf,
}

pub enum DialogAction {
    None,
    Cancel,
    Launch(PathBuf),
}

impl NewChatDialog {
    pub fn new(projects: impl IntoIterator<Item = PathBuf>, preferred: Option<&Path>) -> Self {
        let preferred = preferred.map(normalize);
        let mut projects: BTreeSet<_> = projects.into_iter().map(|path| normalize(&path)).collect();
        if let Some(path) = &preferred {
            projects.insert(path.clone());
        }
        let projects: Vec<_> = projects.into_iter().collect();
        let selected = preferred
            .as_ref()
            .and_then(|path| projects.iter().position(|item| item == path))
            .unwrap_or(0);
        let initial_directory = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            projects,
            selected,
            browser: None,
            error: None,
            initial_directory,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> DialogAction {
        if self.browser.is_some() {
            return self.handle_browser_key(key);
        }
        match key {
            KeyCode::Esc => return DialogAction::Cancel,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(self.projects.len()),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.projects.len(),
            KeyCode::Enter if self.selected < self.projects.len() => {
                return self.launch(self.projects[self.selected].clone());
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Tab => {
                let directory = self
                    .projects
                    .get(self.selected)
                    .cloned()
                    .unwrap_or_else(|| self.initial_directory.clone());
                self.open_browser(&directory);
            }
            _ => {}
        }
        DialogAction::None
    }

    fn handle_browser_key(&mut self, key: KeyCode) -> DialogAction {
        let browser = self.browser.as_mut().expect("browser mode");
        let directory = browser.directory.clone();
        match key {
            KeyCode::Esc | KeyCode::Tab => {
                self.browser = None;
                self.error = None;
            }
            KeyCode::Up => browser.selected = browser.selected.saturating_sub(1),
            KeyCode::Down => browser.selected = (browser.selected + 1).min(browser.row_count() - 1),
            KeyCode::Home => browser.selected = 0,
            KeyCode::End => browser.selected = browser.row_count() - 1,
            KeyCode::Char(' ') => return self.launch(directory),
            KeyCode::Left | KeyCode::Backspace => {
                if let Some(parent) = directory.parent() {
                    self.open_browser(parent);
                }
            }
            KeyCode::Enter if browser.selected == 0 => return self.launch(directory),
            KeyCode::Enter | KeyCode::Right => {
                let target = if browser.has_parent() && browser.selected == 1 {
                    directory.parent().map(Path::to_path_buf)
                } else {
                    browser.selected_child().map(Path::to_path_buf)
                };
                if let Some(path) = target {
                    self.open_browser(&path);
                }
            }
            _ => {}
        }
        DialogAction::None
    }

    fn open_browser(&mut self, path: &Path) {
        match DirectoryBrowser::read(path) {
            Ok(mut browser) => {
                // Preserve the child selection when going up one directory.
                if let Some(previous) = &self.browser
                    && let Some(index) = browser
                        .children
                        .iter()
                        .position(|path| path == &previous.directory)
                {
                    browser.selected = 1 + usize::from(browser.has_parent()) + index;
                }
                self.browser = Some(browser);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!(
                    "Не удалось открыть {}: {error}",
                    display_path(path)
                ));
                // Stale history must not prevent choosing a different directory.
                if self.browser.is_none() {
                    for parent in path
                        .ancestors()
                        .skip(1)
                        .chain(std::iter::once(Path::new("/")))
                    {
                        if let Ok(browser) = DirectoryBrowser::read(parent) {
                            self.browser = Some(browser);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn launch(&mut self, path: PathBuf) -> DialogAction {
        match std::fs::canonicalize(&path) {
            Ok(path) if path.is_dir() => {
                self.error = None;
                DialogAction::Launch(path)
            }
            _ => {
                self.error = Some("Папка недоступна или больше не существует".to_owned());
                DialogAction::None
            }
        }
    }
}

fn normalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "claude-tui-picker-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(std::fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn active_project_is_preselected_even_when_not_in_history() {
        let root = Fixture::new();
        let active = root.0.join("active");
        std::fs::create_dir(&active).unwrap();
        let mut dialog = NewChatDialog::new([root.0.clone(), root.0.clone()], Some(&active));
        assert_eq!(dialog.projects.len(), 2);
        assert_eq!(dialog.projects[dialog.selected], active);
        assert!(
            matches!(dialog.handle_key(KeyCode::Enter), DialogAction::Launch(path) if path == active)
        );
        assert!(dialog.browser.is_none());
    }

    #[test]
    fn browser_visits_directories_and_returns_to_selected_child() {
        let root = Fixture::new();
        let child = root.0.join("Каталог с пробелами");
        std::fs::create_dir(&child).unwrap();
        std::fs::create_dir(root.0.join(".hidden")).unwrap();
        std::fs::write(root.0.join("not-a-directory"), b"file").unwrap();
        let mut dialog = NewChatDialog::new([root.0.clone()], Some(&root.0));
        dialog.handle_key(KeyCode::Tab);
        let browser = dialog.browser.as_mut().unwrap();
        assert_eq!(browser.children.len(), 2);
        let index = browser
            .children
            .iter()
            .position(|path| path == &child)
            .unwrap();
        browser.selected = index + 2;
        dialog.handle_key(KeyCode::Right);
        assert_eq!(dialog.browser.as_ref().unwrap().directory, child);
        dialog.handle_key(KeyCode::Left);
        let browser = dialog.browser.as_ref().unwrap();
        assert_eq!(browser.directory, root.0);
        assert_eq!(browser.selected_child(), Some(child.as_path()));
        dialog.handle_key(KeyCode::Enter);
        assert!(
            matches!(dialog.handle_key(KeyCode::Char(' ')), DialogAction::Launch(path) if path == child)
        );
        dialog.handle_key(KeyCode::Esc);
        assert!(dialog.browser.is_none());
        assert!(matches!(
            dialog.handle_key(KeyCode::Esc),
            DialogAction::Cancel
        ));
    }

    #[test]
    fn stale_history_reports_error_and_browser_can_recover() {
        let root = Fixture::new();
        let missing = root.0.join("removed");
        let mut dialog = NewChatDialog::new([missing.clone()], Some(&missing));
        assert!(matches!(
            dialog.handle_key(KeyCode::Enter),
            DialogAction::None
        ));
        assert!(dialog.error.is_some());
        dialog.handle_key(KeyCode::Tab);
        assert_eq!(dialog.browser.as_ref().unwrap().directory, root.0);
        assert!(matches!(
            dialog.handle_key(KeyCode::Home),
            DialogAction::None
        ));
        assert!(
            matches!(dialog.handle_key(KeyCode::Enter), DialogAction::Launch(path) if path == root.0)
        );
    }

    #[test]
    fn unreadable_child_does_not_discard_current_browser() {
        let root = Fixture::new();
        let child = root.0.join("removed");
        std::fs::create_dir(&child).unwrap();
        let mut dialog = NewChatDialog::new([root.0.clone()], Some(&root.0));
        dialog.handle_key(KeyCode::Tab);
        dialog.browser.as_mut().unwrap().selected = 2;
        std::fs::remove_dir(&child).unwrap();
        dialog.handle_key(KeyCode::Enter);
        assert_eq!(dialog.browser.as_ref().unwrap().directory, root.0);
        assert!(dialog.error.is_some());
        assert!(
            matches!(dialog.handle_key(KeyCode::Char(' ')), DialogAction::Launch(path) if path == root.0)
        );
    }

    #[test]
    fn another_directory_uses_home_instead_of_active_project() {
        let root = Fixture::new();
        let mut dialog = NewChatDialog::new([root.0.clone()], Some(&root.0));
        let expected = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        assert_eq!(dialog.initial_directory, expected);
        // Keep the navigation assertion independent of filesystem permissions on HOME.
        let browser_home = root.0.join("home");
        std::fs::create_dir(&browser_home).unwrap();
        dialog.initial_directory = browser_home.clone();
        dialog.handle_key(KeyCode::End);
        dialog.handle_key(KeyCode::Enter);
        assert_eq!(dialog.browser.as_ref().unwrap().directory, browser_home);
        assert_eq!(dialog.projects[dialog.selected.saturating_sub(1)], root.0);
    }

    #[test]
    fn empty_history_and_root_navigation_have_valid_selection() {
        let mut dialog = NewChatDialog::new([], None);
        dialog.handle_key(KeyCode::Up);
        dialog.handle_key(KeyCode::Down);
        assert_eq!(dialog.selected, 0);
        dialog.open_browser(Path::new("/"));
        dialog.handle_key(KeyCode::Left);
        dialog.handle_key(KeyCode::Backspace);
        let browser = dialog.browser.as_ref().unwrap();
        assert_eq!(browser.directory, Path::new("/"));
        assert!(!browser.has_parent());
        assert!(
            matches!(dialog.handle_key(KeyCode::Enter), DialogAction::Launch(path) if path == Path::new("/"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_are_deduplicated_and_controls_are_not_rendered() {
        let root = Fixture::new();
        let alias = root.0.join("alias");
        std::os::unix::fs::symlink(&root.0, &alias).unwrap();
        let dialog = NewChatDialog::new([root.0.clone(), alias.clone()], Some(&alias));
        assert_eq!(dialog.projects.as_slice(), std::slice::from_ref(&root.0));
        assert_eq!(display_path(Path::new("a\nb\u{1b}")), "a�b�");
    }
}
