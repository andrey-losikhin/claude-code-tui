use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;

use crate::config::{self, UserConfig};
use crate::new_chat::{DialogAction, NewChatDialog};
use crate::sessions::{self, Session};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarRow {
    Project(PathBuf),
    Session(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusPanel {
    Projects,
    Dialogue,
    OpenSessions,
    Notes,
}

pub enum AppAction {
    Resume {
        id: String,
        project_path: PathBuf,
        model: Option<String>,
    },
    NewChat {
        project_path: PathBuf,
        model: Option<String>,
    },
}

pub struct App {
    pub should_quit: bool,
    pub focused_panel: FocusPanel,
    pub sessions: Vec<Session>,
    pub rows: Vec<SidebarRow>,
    pub selected_row: Option<usize>,
    pub selected_session: Option<usize>,
    pub collapsed_projects: HashSet<PathBuf>,
    pub search_query: String,
    pub searching: bool,
    pub renaming: bool,
    pub new_chat_dialog: Option<NewChatDialog>,
    pub selecting_model: bool,
    pub input_buffer: String,
    pub status: String,
    pub help_visible: bool,
    pub help_scroll: u16,
    pub help_scroll_limit: u16,
    config: UserConfig,
    rename_session_id: Option<String>,
    config_writable: bool,
}

impl App {
    pub fn load() -> Self {
        let (config, config_error) = config::load();
        let config_writable = config_error.is_none();
        let collapsed_projects = config.collapsed_projects.clone();
        let mut app = Self {
            should_quit: false,
            focused_panel: FocusPanel::Projects,
            sessions: Vec::new(),
            rows: Vec::new(),
            selected_row: None,
            selected_session: None,
            collapsed_projects,
            search_query: String::new(),
            searching: false,
            renaming: false,
            new_chat_dialog: None,
            selecting_model: false,
            input_buffer: String::new(),
            status: String::new(),
            help_visible: false,
            help_scroll: 0,
            help_scroll_limit: 0,
            config,
            rename_session_id: None,
            config_writable,
        };
        app.reload_sessions();
        if let Some(error) = config_error {
            app.status.push_str(" · ");
            app.status.push_str(&error);
        }
        app
    }

    pub fn reload_sessions(&mut self) {
        self.reload_with_open_sessions(&[], false);
    }

    pub fn sync_live_sessions(&mut self, open: &[crate::pty::OpenSession]) {
        self.reload_with_open_sessions(open, true);
    }

    fn reload_with_open_sessions(
        &mut self,
        open: &[crate::pty::OpenSession],
        preserve_status: bool,
    ) {
        let active_id = self
            .selected_session
            .and_then(|index| self.sessions.get(index))
            .map(|session| session.id.clone());
        let selected_project = self
            .selected_row
            .and_then(|index| self.rows.get(index))
            .and_then(|row| match row {
                SidebarRow::Project(path) => Some(path.clone()),
                SidebarRow::Session(_) => None,
            });
        let (mut sessions, status) = match sessions::default_history_path() {
            Some(path) => match sessions::scan_history(&path) {
                Ok(result) => {
                    let mut status = format!("{} чатов", result.sessions.len());
                    if result.unreadable_files > 0 {
                        status
                            .push_str(&format!(" · пропущено файлов: {}", result.unreadable_files));
                    }
                    (result.sessions, status)
                }
                Err(error) => (Vec::new(), format!("Не удалось прочитать историю: {error}")),
            },
            None => (Vec::new(), "Переменная HOME не задана".to_owned()),
        };
        if preserve_status && status.starts_with("Не удалось") {
            self.status = status;
            return;
        }
        for running in open {
            if !sessions.iter().any(|session| session.id == running.id) {
                let previous = self
                    .sessions
                    .iter()
                    .find(|session| session.id == running.id);
                sessions.push(Session {
                    id: running.id.clone(),
                    project_path: running.project_path.clone(),
                    title: previous
                        .map(|session| session.title.clone())
                        .unwrap_or_else(|| running.title.clone()),
                    custom_title: previous.and_then(|session| session.custom_title.clone()),
                    git_branch: previous.and_then(|session| session.git_branch.clone()),
                    updated_at: previous
                        .map(|session| session.updated_at)
                        .unwrap_or_else(std::time::SystemTime::now),
                });
            }
        }
        let mut renamed_in_cli = false;
        for session in &sessions {
            if session.custom_title.is_some()
                && self
                    .sessions
                    .iter()
                    .find(|old| old.id == session.id)
                    .is_some_and(|old| old.custom_title != session.custom_title)
            {
                renamed_in_cli |= self.config.renamed_sessions.remove(&session.id).is_some();
            }
        }
        if renamed_in_cli {
            self.save_config();
        }
        let unchanged = self.sessions.len() == sessions.len()
            && self.sessions.iter().zip(&sessions).all(|(old, new)| {
                old.id == new.id
                    && old.project_path == new.project_path
                    && old.title == new.title
                    && old.custom_title == new.custom_title
                    && old.git_branch == new.git_branch
                    && old.updated_at == new.updated_at
            });
        if preserve_status && unchanged {
            return;
        }
        self.sessions = sessions;
        self.selected_session =
            active_id.and_then(|id| self.sessions.iter().position(|session| session.id == id));
        if !preserve_status {
            self.status = status;
        }
        self.rows.clear();
        self.selected_row = None;
        self.refresh_rows();
        if let Some(path) = selected_project {
            self.selected_row = self
                .rows
                .iter()
                .position(|row| matches!(row, SidebarRow::Project(row_path) if row_path == &path));
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Option<AppAction> {
        if let Some(dialog) = &mut self.new_chat_dialog {
            return match dialog.handle_key(key) {
                DialogAction::None => None,
                DialogAction::Cancel => {
                    self.new_chat_dialog = None;
                    None
                }
                DialogAction::Launch(project_path) => Some(AppAction::NewChat {
                    project_path,
                    model: self.current_model(),
                }),
            };
        }
        if self.help_visible {
            match key {
                KeyCode::Esc | KeyCode::Char('?') => self.help_visible = false,
                KeyCode::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
                KeyCode::Down => {
                    self.help_scroll = self
                        .help_scroll
                        .saturating_add(1)
                        .min(self.help_scroll_limit);
                }
                _ => {}
            }
            return None;
        }
        if self.renaming || self.selecting_model {
            return self.handle_text_input(key);
        }
        if self.searching {
            match key {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search_query.clear();
                    self.refresh_rows();
                }
                KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.refresh_rows();
                }
                KeyCode::Char(character) => {
                    self.search_query.push(character);
                    self.refresh_rows();
                }
                _ => {}
            }
            return None;
        }

        match key {
            KeyCode::Char(character) if Self::shortcut_character(character) == 'q' => {
                self.should_quit = true
            }
            KeyCode::Char('?') => {
                self.help_scroll = 0;
                self.help_visible = true;
            }
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char(character) if Self::shortcut_character(character) == 'f' => {
                self.searching = true
            }
            KeyCode::Esc => {
                self.search_query.clear();
                self.refresh_rows();
            }
            KeyCode::Up | KeyCode::Down => self.move_selection(key == KeyCode::Down),
            KeyCode::Left => self.set_selected_project_collapsed(true),
            KeyCode::Right => self.set_selected_project_collapsed(false),
            KeyCode::Enter => return self.activate_selected_row(),
            KeyCode::Char(character) if Self::shortcut_character(character) == 'r' => {
                self.begin_rename()
            }
            KeyCode::Char(character) if Self::shortcut_character(character) == 'p' => {
                self.toggle_pin()
            }
            KeyCode::Char(character) if Self::shortcut_character(character) == 'x' => {
                self.hide_selected_project()
            }
            KeyCode::Char(character) if Self::shortcut_character(character) == 'm' => {
                self.selecting_model = true;
                self.input_buffer = self.config.selected_model.clone().unwrap_or_default();
            }
            KeyCode::Char(character) if Self::shortcut_character(character) == 'o' => {
                return self.resume_action();
            }
            _ => {}
        }
        None
    }

    pub fn shortcut_character(character: char) -> char {
        let lower = character.to_lowercase().next().unwrap_or(character);
        match lower {
            'й' => 'q',
            'ц' => 'w',
            'у' => 'e',
            'к' => 'r',
            'е' => 't',
            'н' => 'y',
            'г' => 'u',
            'ш' => 'i',
            'щ' => 'o',
            'з' => 'p',
            'ф' => 'a',
            'ы' => 's',
            'в' => 'd',
            'а' => 'f',
            'п' => 'g',
            'р' => 'h',
            'о' => 'j',
            'л' => 'k',
            'д' => 'l',
            'я' => 'z',
            'ч' => 'x',
            'с' => 'c',
            'м' => 'v',
            'и' => 'b',
            'т' => 'n',
            'ь' => 'm',
            other => other,
        }
    }

    pub fn input_active(&self) -> bool {
        self.searching || self.renaming || self.new_chat_dialog.is_some() || self.selecting_model
    }

    pub fn cancel_input_modes(&mut self) {
        self.searching = false;
        self.search_query.clear();
        self.renaming = false;
        self.new_chat_dialog = None;
        self.selecting_model = false;
        self.rename_session_id = None;
        self.input_buffer.clear();
        self.refresh_rows();
    }

    fn refresh_rows(&mut self) {
        let previous_selection = self
            .selected_row
            .and_then(|index| self.rows.get(index))
            .cloned();
        let query = self.search_query.to_lowercase();
        let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();

        for (index, session) in self.sessions.iter().enumerate() {
            if self.config.hidden_projects.contains(&session.project_path) {
                continue;
            }
            if !query.is_empty()
                && !self.display_title(index).to_lowercase().contains(&query)
                && !session
                    .project_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query)
            {
                continue;
            }

            groups
                .entry(session.project_path.clone())
                .or_default()
                .push(index);
        }

        for project in &self.config.projects {
            if !self.config.hidden_projects.contains(project)
                && (query.is_empty() || project.to_string_lossy().to_lowercase().contains(&query))
            {
                groups.entry(project.clone()).or_default();
            }
        }

        for indices in groups.values_mut() {
            indices.sort_by_key(|index| {
                !self
                    .sessions
                    .get(*index)
                    .is_some_and(|session| self.config.pinned_sessions.contains(&session.id))
            });
        }

        self.rows.clear();
        for (project_path, session_indices) in groups {
            self.rows.push(SidebarRow::Project(project_path.clone()));
            if !self.collapsed_projects.contains(&project_path) || !query.is_empty() {
                self.rows
                    .extend(session_indices.into_iter().map(SidebarRow::Session));
            }
        }

        self.selected_row = previous_selection
            .and_then(|previous| self.rows.iter().position(|row| row == &previous))
            .or_else(|| {
                self.rows.iter().position(|row| {
                    matches!(row, SidebarRow::Session(index) if Some(*index) == self.selected_session)
                })
            })
            .or_else(|| (!self.rows.is_empty()).then_some(0));

        if let Some(SidebarRow::Session(index)) = self
            .selected_row
            .and_then(|selected| self.rows.get(selected))
        {
            self.selected_session = Some(*index);
        }
    }

    fn move_selection(&mut self, down: bool) {
        if self.rows.is_empty() {
            self.selected_row = None;
            return;
        }

        let current = self.selected_row.unwrap_or(0);
        let next = match down {
            true => (current + 1) % self.rows.len(),
            false => (current + self.rows.len() - 1) % self.rows.len(),
        };
        self.selected_row = Some(next);
        if let SidebarRow::Session(index) = &self.rows[next] {
            self.selected_session = Some(*index);
        }
    }

    fn selected_project_path(&self) -> Option<PathBuf> {
        match self.selected_row.and_then(|index| self.rows.get(index))? {
            SidebarRow::Project(path) => Some(path.clone()),
            SidebarRow::Session(index) => self
                .sessions
                .get(*index)
                .map(|session| session.project_path.clone()),
        }
    }

    fn set_selected_project_collapsed(&mut self, collapsed: bool) {
        let Some(project_path) = self.selected_project_path() else {
            return;
        };
        if collapsed {
            self.collapsed_projects.insert(project_path.clone());
        } else {
            self.collapsed_projects.remove(&project_path);
        }
        self.config.collapsed_projects = self.collapsed_projects.clone();
        self.save_config();
        self.refresh_rows();
        if collapsed {
            self.selected_row = self
                .rows
                .iter()
                .position(|row| matches!(row, SidebarRow::Project(path) if path == &project_path));
        }
    }

    fn activate_selected_row(&mut self) -> Option<AppAction> {
        match self
            .selected_row
            .and_then(|index| self.rows.get(index))
            .cloned()
        {
            Some(SidebarRow::Project(path)) => {
                if self.collapsed_projects.contains(&path) {
                    self.collapsed_projects.remove(&path);
                } else {
                    self.collapsed_projects.insert(path);
                }
                self.config.collapsed_projects = self.collapsed_projects.clone();
                self.save_config();
                self.refresh_rows();
                None
            }
            Some(SidebarRow::Session(index)) => {
                self.selected_session = Some(index);
                self.resume_action()
            }
            None => None,
        }
    }

    fn resume_action(&self) -> Option<AppAction> {
        let selected_row = self.selected_row.and_then(|index| self.rows.get(index))?;
        let index = match selected_row {
            SidebarRow::Session(index) => *index,
            SidebarRow::Project(path) => self
                .selected_session
                .filter(|index| {
                    self.sessions
                        .get(*index)
                        .is_some_and(|s| &s.project_path == path)
                })
                .or_else(|| {
                    self.sessions
                        .iter()
                        .position(|session| &session.project_path == path)
                })?,
        };
        let session = self.sessions.get(index)?;
        Some(AppAction::Resume {
            id: session.id.clone(),
            project_path: session.project_path.clone(),
            model: self.current_model(),
        })
    }

    pub fn begin_new_chat(&mut self, active_project: Option<&Path>, open_projects: Vec<PathBuf>) {
        let selected_project = self.selected_project_path();
        let preferred = active_project.or(selected_project.as_deref());
        let projects = self
            .config
            .projects
            .iter()
            .cloned()
            .chain(
                self.sessions
                    .iter()
                    .map(|session| session.project_path.clone()),
            )
            .chain(open_projects);
        self.new_chat_dialog = Some(NewChatDialog::new(projects, preferred));
        self.help_visible = false;
    }

    pub fn finish_new_chat(&mut self, project_path: PathBuf) -> Option<String> {
        self.new_chat_dialog = None;
        self.search_query.clear();
        self.config.hidden_projects.remove(&project_path);
        self.collapsed_projects.remove(&project_path);
        self.config.collapsed_projects = self.collapsed_projects.clone();
        self.config.projects.insert(project_path);
        let saved = self.save_config();
        self.refresh_rows();
        (!saved).then(|| self.status.clone())
    }

    pub fn display_title(&self, index: usize) -> &str {
        let Some(session) = self.sessions.get(index) else {
            return "";
        };
        self.config
            .renamed_sessions
            .get(&session.id)
            .map(String::as_str)
            .unwrap_or(&session.title)
    }

    pub fn is_pinned(&self, index: usize) -> bool {
        self.sessions
            .get(index)
            .is_some_and(|session| self.config.pinned_sessions.contains(&session.id))
    }

    fn begin_rename(&mut self) {
        let selected_index = match self.selected_row.and_then(|index| self.rows.get(index)) {
            Some(SidebarRow::Session(index)) => Some(*index),
            Some(SidebarRow::Project(path)) => self.selected_session.filter(|index| {
                self.sessions
                    .get(*index)
                    .is_some_and(|session| &session.project_path == path)
            }),
            None => self.selected_session,
        };
        let Some(index) = selected_index else {
            self.status = "Выберите чат для переименования".to_owned();
            return;
        };
        let Some(session) = self.sessions.get(index) else {
            return;
        };
        self.rename_session_id = Some(session.id.clone());
        self.input_buffer = self.display_title(index).to_owned();
        self.renaming = true;
    }

    fn handle_text_input(&mut self, key: KeyCode) -> Option<AppAction> {
        match key {
            KeyCode::Esc => {
                self.renaming = false;
                self.selecting_model = false;
                self.rename_session_id = None;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Enter if self.renaming => self.commit_rename(),
            KeyCode::Enter if self.selecting_model => self.commit_model(),
            KeyCode::Char(character) => self.input_buffer.push(character),
            _ => {}
        }
        None
    }

    fn commit_rename(&mut self) {
        let Some(id) = self.rename_session_id.take() else {
            self.renaming = false;
            return;
        };
        let title = self.input_buffer.trim().to_owned();
        if title.is_empty() {
            self.rename_session_id = Some(id);
            self.status = "Название чата не может быть пустым".to_owned();
            return;
        }
        self.config.renamed_sessions.insert(id, title);
        self.renaming = false;
        self.input_buffer.clear();
        self.save_config();
        self.refresh_rows();
    }

    fn commit_model(&mut self) {
        let model = self.input_buffer.trim().to_owned();
        self.config.selected_model = (!model.is_empty()).then_some(model);
        self.selecting_model = false;
        self.input_buffer.clear();
        self.save_config();
    }

    fn current_model(&self) -> Option<String> {
        self.config
            .selected_model
            .as_ref()
            .filter(|model| !model.trim().is_empty())
            .cloned()
    }

    fn toggle_pin(&mut self) {
        let selected_index = match self.selected_row.and_then(|index| self.rows.get(index)) {
            Some(SidebarRow::Session(index)) => Some(*index),
            Some(SidebarRow::Project(path)) => self.selected_session.filter(|index| {
                self.sessions
                    .get(*index)
                    .is_some_and(|session| &session.project_path == path)
            }),
            None => self.selected_session,
        };
        let Some(id) = selected_index
            .and_then(|index| self.sessions.get(index))
            .map(|session| session.id.clone())
        else {
            self.status = "Выберите чат для закрепления".to_owned();
            return;
        };
        if !self.config.pinned_sessions.remove(&id) {
            self.config.pinned_sessions.insert(id);
        }
        self.save_config();
        self.refresh_rows();
    }

    fn hide_selected_project(&mut self) {
        let Some(path) = self.selected_project_path() else {
            return;
        };
        let previous_config = self.config.clone();
        let previous_collapsed = self.collapsed_projects.clone();

        self.config.hidden_projects.insert(path.clone());
        self.config.projects.remove(&path);
        self.collapsed_projects.remove(&path);
        self.config.collapsed_projects = self.collapsed_projects.clone();

        if !self.save_config() {
            self.config = previous_config;
            self.collapsed_projects = previous_collapsed;
            return;
        }

        if self
            .selected_session
            .and_then(|index| self.sessions.get(index))
            .is_some_and(|session| session.project_path == path)
        {
            self.selected_session = None;
        }
        self.refresh_rows();
        self.status = format!("Проект скрыт: {}", path.display());
    }

    pub fn title_for_id(&self, id: &str) -> Option<&str> {
        self.config
            .renamed_sessions
            .get(id)
            .map(String::as_str)
            .or_else(|| {
                self.sessions
                    .iter()
                    .find(|session| session.id == id)
                    .map(|session| session.title.as_str())
            })
    }

    pub fn display_title_for_id(&self, id: &str) -> String {
        self.title_for_id(id).unwrap_or(id).to_owned()
    }

    pub fn select_session_id(&mut self, id: Option<&str>, project_path: Option<&Path>) {
        let index = id.and_then(|id| self.sessions.iter().position(|session| session.id == id));
        self.selected_session = index;
        self.selected_row = index
            .and_then(|index| {
                self.rows.iter().position(
                    |row| matches!(row, SidebarRow::Session(row_index) if *row_index == index),
                )
            })
            .or_else(|| {
                project_path.and_then(|path| {
                    self.rows.iter().position(
                        |row| matches!(row, SidebarRow::Project(row_path) if row_path == path),
                    )
                })
            });
    }

    fn save_config(&mut self) -> bool {
        if !self.config_writable {
            self.status = "Настройки не сохранены: конфиг повреждён или недоступен".to_owned();
            return false;
        }
        match config::save(&self.config) {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Не удалось сохранить настройки: {error}");
                false
            }
        }
    }
}
