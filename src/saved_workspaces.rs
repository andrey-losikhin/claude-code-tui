//! Named local workspace snapshots; no transcript or editor buffer copies.
use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::{
    app::{App, FocusPanel},
    config::LayoutConfig,
    notes::NotesManager,
    pty::PtyManager,
    workspace::{Entry, Kind, Popup, Target},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub project: PathBuf,
    pub title: String,
    pub transcript: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub chats: Vec<Chat>,
    pub active_chat: Option<String>,
    pub visible_notes: Vec<String>,
    pub layout: LayoutConfig,
    pub maximized: bool,
    pub focus: FocusPanel,
    pub model: Option<String>,
}
impl Snapshot {
    fn capture(name: String, app: &App, processes: &PtyManager, notes: &NotesManager) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            chats: processes
                .open_sessions()
                .into_iter()
                .map(|chat| Chat {
                    transcript: app
                        .sessions
                        .iter()
                        .find(|s| s.id == chat.id)
                        .and_then(|s| s.history_path.clone()),
                    id: chat.id,
                    project: chat.project_path,
                    title: chat.title,
                })
                .collect(),
            active_chat: processes.active_id().map(str::to_owned),
            visible_notes: notes.visible_ids(),
            layout: app.saved_layout(),
            maximized: app.maximized,
            focus: app.focused_panel,
            model: app.workspace_model(),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.chats.len() > 256 || uuid::Uuid::parse_str(&self.id).is_err() {
            return Err("Повреждён снимок сессии".into());
        }
        let mut ids = HashSet::new();
        for chat in &self.chats {
            if uuid::Uuid::parse_str(&chat.id).is_err()
                || !ids.insert(&chat.id)
                || !chat.project.is_absolute()
            {
                return Err("Некорректный UUID или путь в сессии".into());
            }
        }
        if self
            .active_chat
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
            || self.visible_notes.iter().any(|id| !ids.contains(id))
        {
            return Err("Сессия ссылается на отсутствующий чат".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Menu(String),
    SaveAs,
    Save,
    SaveClose,
    CloseAsk,
    Close,
    RestoreAsk(String),
    Restore(String),
    Rename(String),
    DeleteAsk(String),
    Delete(String),
    Name(String),
    Back,
}
enum Naming {
    Save { close: bool },
    Rename(String),
}
enum Pending {
    Close,
    Restore(Snapshot),
}
#[derive(Default)]
pub struct Manager {
    active: Option<String>,
    naming: Option<Naming>,
    pending: Option<Pending>,
}
fn entry(label: impl Into<String>, action: Action) -> Entry {
    Entry {
        label: label.into(),
        target: Target::Workspace(action),
    }
}
fn show(app: &mut App, entries: Vec<Entry>, hint: &str) {
    let mut popup = Popup::new(Kind::Workspaces, entries);
    popup.hint = hint.into();
    app.popup = Some(popup);
}
fn valid_name(name: &str, items: &[Snapshot], excluding: Option<&str>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Название: от 1 до 80 символов без управляющих знаков".into());
    }
    if items.iter().any(|item| {
        Some(item.id.as_str()) != excluding && item.name.to_lowercase() == name.to_lowercase()
    }) {
        return Err("Сессия с таким названием уже есть".into());
    }
    Ok(name.into())
}
impl Manager {
    pub fn open(&mut self, app: &mut App) {
        self.naming = None;
        app.cancel_input_modes();
        app.help_visible = false;
        let mut entries = vec![entry("Сохранить как новую сессию", Action::SaveAs)];
        if self
            .active
            .as_ref()
            .is_some_and(|id| app.workspaces().iter().any(|s| &s.id == id))
        {
            entries.push(entry("Обновить текущую сохранённую сессию", Action::Save));
        }
        entries.push(entry("Сохранить и закрыть текущие чаты", Action::SaveClose));
        entries.push(entry(
            "Закрыть текущие чаты без обновления снимка",
            Action::CloseAsk,
        ));
        entries.extend(app.workspaces().iter().map(|s| {
            entry(
                format!(
                    "{}{} · {} чатов",
                    if self.active.as_ref() == Some(&s.id) {
                        "● "
                    } else {
                        ""
                    },
                    s.name,
                    s.chats.len()
                ),
                Action::Menu(s.id.clone()),
            )
        }));
        show(
            app,
            entries,
            "Поиск · Enter выбрать · закрытие остановит CLI",
        );
    }
    fn ask_name(&mut self, app: &mut App, naming: Naming, initial: String) {
        self.naming = Some(naming);
        let mut popup = Popup::new(Kind::WorkspaceName, vec![]);
        popup.query = initial;
        popup.hint = "Введите название · Enter сохранить".into();
        app.popup = Some(popup);
    }
    fn confirm(app: &mut App, label: String, action: Action) {
        show(
            app,
            vec![entry("Отмена", Action::Back), entry(label, action)],
            "Подтверждение · выберите действие",
        );
    }
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }
    pub fn cancel_from_editor_key(
        &mut self,
        key: &crossterm::event::KeyEvent,
        app: &App,
        notes: &mut NotesManager,
    ) {
        use crossterm::event::KeyCode;
        if self.pending.is_some()
            && notes.exit_requested
            && app.focused_panel == FocusPanel::Notes
            && key.modifiers.is_empty()
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('c' | 'C'))
        {
            // Still forward the key so Neovim dismisses its own confirmation.
            self.pending = None;
            notes.cancel_exit();
        }
    }
    pub fn cancel_if_needed(&mut self, notes: &NotesManager) {
        if !notes.exit_requested {
            self.pending = None;
        }
    }
    pub fn handle(
        &mut self,
        action: Action,
        app: &mut App,
        processes: &mut PtyManager,
        notes: &mut NotesManager,
        area: Rect,
    ) {
        if let Err(error) = self.perform(action, app, processes, notes, area) {
            app.status = error;
        }
    }
    fn save(
        &mut self,
        app: &mut App,
        processes: &PtyManager,
        notes: &NotesManager,
        name: String,
        existing: Option<String>,
    ) -> Result<(), String> {
        let name = valid_name(&name, app.workspaces(), existing.as_deref())?;
        let mut snapshot = Snapshot::capture(name, app, processes, notes);
        // Only visible notes belonging to this set of chats are included.
        snapshot
            .visible_notes
            .retain(|id| snapshot.chats.iter().any(|c| &c.id == id));
        snapshot.validate()?;
        let mut items = app.workspaces().to_vec();
        if let Some(id) = existing {
            snapshot.id = id.clone();
            let index = items
                .iter()
                .position(|s| s.id == id)
                .ok_or("Сессия не найдена")?;
            items[index] = snapshot.clone();
        } else {
            if items.len() >= 128 {
                return Err("Достигнут лимит: 128 сохранённых сессий".into());
            }
            items.push(snapshot.clone());
        }
        if !app.store_workspaces(items) {
            return Err(app.status.clone());
        }
        self.active = Some(snapshot.id);
        app.status = format!("Сессия сохранена: {}", snapshot.name);
        Ok(())
    }
    fn perform(
        &mut self,
        action: Action,
        app: &mut App,
        processes: &mut PtyManager,
        notes: &mut NotesManager,
        _area: Rect,
    ) -> Result<(), String> {
        match action {
            Action::Back => self.open(app),
            Action::Menu(id) => {
                let item = app
                    .workspaces()
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or("Сессия не найдена")?;
                show(
                    app,
                    vec![
                        entry(
                            format!("Открыть «{}»", item.name),
                            Action::RestoreAsk(id.clone()),
                        ),
                        entry("Переименовать", Action::Rename(id.clone())),
                        entry("Удалить сохранённый набор", Action::DeleteAsk(id)),
                        entry("Назад", Action::Back),
                    ],
                    "Истории и заметки сохраняются",
                );
            }
            Action::SaveAs => self.ask_name(app, Naming::Save { close: false }, String::new()),
            Action::Save | Action::SaveClose => {
                let close = matches!(action, Action::SaveClose);
                let existing = self
                    .active
                    .as_ref()
                    .and_then(|id| app.workspaces().iter().find(|s| &s.id == id))
                    .cloned();
                if let Some(item) = existing {
                    self.save(app, processes, notes, item.name, Some(item.id))?;
                    if close {
                        Self::confirm(app, "Закрыть чаты (остановить CLI)".into(), Action::Close);
                    }
                } else {
                    self.ask_name(app, Naming::Save { close }, String::new());
                }
            }
            Action::Rename(id) => {
                let name = app
                    .workspaces()
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or("Сессия не найдена")?
                    .name
                    .clone();
                self.ask_name(app, Naming::Rename(id), name);
            }
            Action::Name(name) => {
                let naming = self.naming.take().ok_or("Нет операции ввода имени")?;
                let name = match valid_name(
                    &name,
                    app.workspaces(),
                    match &naming {
                        Naming::Rename(id) => Some(id),
                        _ => None,
                    },
                ) {
                    Ok(name) => name,
                    Err(error) => {
                        self.ask_name(app, naming, name);
                        return Err(error);
                    }
                };
                match naming {
                    Naming::Save { close } => {
                        self.save(app, processes, notes, name, None)?;
                        if close {
                            Self::confirm(
                                app,
                                "Закрыть чаты (остановить CLI)".into(),
                                Action::Close,
                            );
                        }
                    }
                    Naming::Rename(id) => {
                        let mut items = app.workspaces().to_vec();
                        items
                            .iter_mut()
                            .find(|s| s.id == id)
                            .ok_or("Сессия не найдена")?
                            .name = name;
                        if !app.store_workspaces(items) {
                            return Err(app.status.clone());
                        }
                        self.open(app);
                        app.status = "Сессия переименована".into();
                    }
                }
            }
            Action::DeleteAsk(id) => Self::confirm(
                app,
                "Удалить снимок (чаты и заметки остаются)".into(),
                Action::Delete(id),
            ),
            Action::Delete(id) => {
                let mut items = app.workspaces().to_vec();
                items.retain(|s| s.id != id);
                if !app.store_workspaces(items) {
                    return Err(app.status.clone());
                }
                if self.active.as_ref() == Some(&id) {
                    self.active = None;
                }
                self.open(app);
                app.status = "Сохранённый набор удалён · истории и заметки остаются".into();
            }
            Action::CloseAsk => {
                Self::confirm(app, "Закрыть чаты (остановить CLI)".into(), Action::Close)
            }
            Action::RestoreAsk(id) => Self::confirm(
                app,
                "Закрыть текущие чаты и восстановить сессию".into(),
                Action::Restore(id),
            ),
            Action::Close | Action::Restore(_) => {
                let pending = if let Action::Restore(id) = action {
                    let snapshot = app
                        .workspaces()
                        .iter()
                        .find(|s| s.id == id)
                        .ok_or("Сессия не найдена")?
                        .clone();
                    snapshot.validate()?;
                    for chat in &snapshot.chats {
                        if !chat.project.is_dir() {
                            return Err(format!("Папка недоступна: {}", chat.project.display()));
                        }
                    }
                    Pending::Restore(snapshot)
                } else {
                    Pending::Close
                };
                notes.request_exit().map_err(|e| {
                    notes.cancel_exit();
                    e.to_string()
                })?;
                self.pending = Some(pending);
                app.maximized = false;
                app.focused_panel = FocusPanel::Notes;
                app.status = "Закрытие: подтвердите сохранение в nvim · Alt+↑ отменить".into();
            }
        }
        Ok(())
    }
    /// Called only after all note editors have exited through Neovim confirmation.
    pub fn finish(
        &mut self,
        app: &mut App,
        processes: &mut PtyManager,
        notes: &mut NotesManager,
        bounds: Rect,
    ) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        notes.cancel_exit();
        for id in processes.running_ids() {
            processes.select_active(Some(&id));
            if let Err(error) = processes.close_active() {
                app.status = format!("Не удалось закрыть чат: {error}");
                app.focus_projects();
                return true;
            }
        }
        *notes = NotesManager::default();
        app.sync_live_sessions(&processes.open_sessions());
        app.selected_session = None;
        app.focus_projects();
        match pending {
            Pending::Close => {
                self.active = None;
                app.status = "Рабочая сессия закрыта · истории и заметки сохранены".into();
            }
            Pending::Restore(snapshot) => {
                let (chat_area, note_area) =
                    crate::ui::configured_dialog_areas(bounds, true, &snapshot.layout);
                let mut errors = Vec::new();
                for chat in &snapshot.chats {
                    let transcript = app
                        .sessions
                        .iter()
                        .find(|s| s.id == chat.id)
                        .and_then(|s| s.history_path.as_ref())
                        .or(chat.transcript.as_ref());
                    let resume = transcript.is_some_and(|p| p.is_file());
                    match processes.start(
                        Some(&chat.id),
                        chat.title.clone(),
                        &chat.project,
                        resume.then_some(chat.id.as_str()),
                        snapshot.model.as_deref(),
                        chat_area,
                    ) {
                        Ok(_) => {
                            if snapshot.visible_notes.contains(&chat.id)
                                && let Err(error) = notes.open(
                                    &chat.id,
                                    &chat.project,
                                    note_area.unwrap_or(chat_area),
                                )
                            {
                                errors.push(format!("Заметка {}: {error}", chat.id));
                            }
                        }
                        Err(error) => errors.push(format!("Чат {}: {error}", chat.id)),
                    }
                }
                if snapshot
                    .active_chat
                    .as_deref()
                    .is_some_and(|id| processes.is_running(id))
                {
                    processes.select_active(snapshot.active_chat.as_deref());
                }
                app.sync_live_sessions(&processes.open_sessions());
                app.select_session_id(processes.active_id(), processes.active_project_path());
                if let Some(warning) = app.apply_workspace(&snapshot) {
                    errors.push(warning);
                }
                self.active = Some(snapshot.id);
                app.status = format!(
                    "Сессия восстановлена: {}{}",
                    snapshot.name,
                    if errors.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", errors.join("; "))
                    }
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_bounded_and_unique() {
        assert!(valid_name("   ", &[], None).is_err());
        assert!(valid_name("a\nb", &[], None).is_err());
        assert!(valid_name(&"a".repeat(81), &[], None).is_err());
        assert_eq!(valid_name(" Work ", &[], None).unwrap(), "Work");
    }
    #[test]
    fn snapshots_roundtrip_and_reject_unknown_chats() {
        let mut snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Work".into(),
            chats: vec![],
            active_chat: None,
            visible_notes: vec![],
            layout: LayoutConfig::default(),
            maximized: false,
            focus: FocusPanel::Projects,
            model: None,
        };
        snapshot.validate().unwrap();
        let restored: Snapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(restored.name, "Work");
        assert!(valid_name("work", &[restored], None).is_err());
        snapshot.visible_notes.push("../../bad".into());
        assert!(snapshot.validate().is_err());
    }
}
