use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, FocusPanel, SidebarRow};
use crate::new_chat::{NewChatDialog, display_path};
use crate::notes::NotesManager;
use crate::pty::OpenSession;
use crate::theme::Theme;

#[derive(Default)]
pub struct MouseLayout {
    pub projects: Rect,
    pub sessions: Rect,
    pub project_offset: usize,
    pub session_offset: usize,
    pub picker: Option<(Rect, usize)>,
}

pub fn chat_area(area: Rect) -> Rect {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(areas[0]);
    columns[1]
}

pub fn dialog_areas(area: Rect, show_note: bool) -> (Rect, Option<Rect>) {
    let dialog = chat_area(area);
    if !show_note {
        return (dialog, None);
    }
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(dialog);
    (split[0], Some(split[1]))
}

#[expect(
    clippy::too_many_arguments,
    reason = "The frame renderer combines existing UI, CLI and editor state"
)]
pub fn render(
    frame: &mut Frame,
    app: &mut App,
    theme: Theme,
    process_lines: &[Line<'static>],
    open_sessions: &[OpenSession],
    active_title: Option<&str>,
    active_cursor: Option<(u16, u16)>,
    notes: &NotesManager,
) -> MouseLayout {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(areas[0]);
    let left_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(6)])
        .split(columns[0]);
    let (chat_area, note_area) = dialog_areas(frame.area(), notes.is_visible());

    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| match row {
            SidebarRow::Project(path) => {
                let (marker, icon) = if app.collapsed_projects.contains(path) {
                    ("▸", "")
                } else {
                    ("▾", "")
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{marker} "), theme.project_heading()),
                    Span::styled(format!("{icon}  "), theme.project_icon()),
                    Span::styled(project_name(path), theme.project_heading()),
                ]))
            }
            SidebarRow::Session(index) => {
                let session_id = app.sessions.get(*index).map(|session| session.id.as_str());
                let open = open_sessions
                    .iter()
                    .find(|open_session| Some(open_session.id.as_str()) == session_id);
                let icon = match open {
                    Some(session) if session.active => "●",
                    Some(session) if session.running => "◉",
                    Some(_) => "×",
                    None => "",
                };
                let icon_style = match open {
                    Some(session) if session.active || session.running => {
                        theme.session_icon(session.active)
                    }
                    Some(_) => theme.stopped_icon(),
                    None => theme.history_icon(),
                };
                let pinned = if app.is_pinned(*index) { " " } else { "" };
                let note = if session_id.is_some_and(|id| notes.has_note(id)) {
                    "✎ "
                } else {
                    ""
                };
                ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(format!("{icon}  "), icon_style),
                    Span::styled(pinned, theme.project_icon()),
                    Span::styled(note, theme.project_icon()),
                    Span::raw(app.display_title(*index)),
                ]))
            }
        })
        .collect();

    let mut list_state = ListState::default().with_selected(app.selected_row);
    let sidebar_title = if app.renaming {
        format!("Новое название: {}", app.input_buffer)
    } else if app.selecting_model {
        format!("Модель (пусто = default): {}", app.input_buffer)
    } else if app.searching || !app.search_query.is_empty() {
        format!("Поиск: {}", app.search_query)
    } else {
        "Проекты · f поиск · n новый чат".to_owned()
    };
    let sidebar = List::new(items)
        .highlight_style(theme.selected_row())
        .block(
            Block::bordered()
                .border_style(theme.focused_border(app.focused_panel == FocusPanel::Projects))
                .title_style(theme.focused_title(app.focused_panel == FocusPanel::Projects))
                .title(sidebar_title),
        );
    frame.render_stateful_widget(sidebar, left_areas[0], &mut list_state);

    let open_items: Vec<ListItem> = if open_sessions.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Пока нет открытых чатов",
            theme.history_icon(),
        )))]
    } else {
        open_sessions
            .iter()
            .map(|session| {
                let (icon, style) = if session.active {
                    ("●", theme.session_icon(true))
                } else if session.running {
                    ("◉", theme.session_icon(false))
                } else {
                    ("×", theme.stopped_icon())
                };
                let project = project_name(&session.project_path);
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{icon}  "), style),
                    Span::styled(
                        if notes.has_note(&session.id) {
                            "✎ "
                        } else {
                            ""
                        },
                        theme.project_icon(),
                    ),
                    Span::raw(format!("{} · {project}", session.title)),
                ]))
            })
            .collect()
    };
    let mut open_state =
        ListState::default().with_selected(open_sessions.iter().position(|session| session.active));
    let open_list = List::new(open_items)
        .highlight_style(theme.selected_row())
        .block(
            Block::bordered()
                .title_style(theme.focused_title(app.focused_panel == FocusPanel::OpenSessions))
                .border_style(theme.focused_border(app.focused_panel == FocusPanel::OpenSessions))
                .title("Открытые чаты · ↑/↓ выбрать"),
        );
    frame.render_stateful_widget(open_list, left_areas[1], &mut open_state);

    let chat_text = match app
        .selected_session
        .and_then(|index| app.sessions.get(index).map(|session| (index, session)))
    {
        Some((index, session)) => vec![
            Line::from(Span::styled(app.display_title(index), theme.active_chat())),
            Line::from(format!("Проект: {}", session.project_path.display())),
            Line::from(format!("Сессия: {}", session.id)),
            Line::from(format!(
                "Git: {}",
                session.git_branch.as_deref().unwrap_or("—")
            )),
            Line::from(""),
            Line::from("Нажмите Enter или o, чтобы открыть CLI-сессию."),
        ],
        None => vec![
            Line::from(if app.sessions.is_empty() {
                "Нажмите n в списке или Alt+N, чтобы создать новый чат."
            } else {
                "Выберите чат в списке и нажмите Enter."
            }),
            Line::from(""),
            Line::from("Папка выбирается в окне нового чата; q — выход."),
        ],
    };
    let chat_title = active_title
        .map(|title| format!("Claude Code · {title}"))
        .unwrap_or_else(|| "Чат".to_owned());
    let chat_widget = if active_title.is_some() {
        Paragraph::new(process_lines.to_vec())
    } else {
        Paragraph::new(chat_text)
    };
    let chat = chat_widget.block(
        Block::bordered()
            .border_style(theme.focused_border(app.focused_panel == FocusPanel::Dialogue))
            .title_style(theme.focused_title(app.focused_panel == FocusPanel::Dialogue))
            .title(chat_title),
    );
    frame.render_widget(chat, chat_area);
    if app.focused_panel == FocusPanel::Dialogue
        && !app.help_visible
        && app.new_chat_dialog.is_none()
        && let Some((row, col)) = active_cursor
    {
        set_pty_cursor(frame, chat_area, row, col);
    }
    if let Some(area) = note_area {
        let title = notes
            .active_path()
            .map(|path| {
                let name = path.file_name().map(Path::new).unwrap_or(&path);
                format!("✎ Заметка · {} · Alt+↓", display_path(name))
            })
            .unwrap_or_else(|| "✎ Заметка · Alt+↓".to_owned());
        frame.render_widget(
            Paragraph::new(notes.lines()).block(
                Block::bordered()
                    .title(title)
                    .title_style(theme.focused_title(app.focused_panel == FocusPanel::Notes))
                    .border_style(theme.focused_border(app.focused_panel == FocusPanel::Notes)),
            ),
            area,
        );
        if app.focused_panel == FocusPanel::Notes
            && !app.help_visible
            && app.new_chat_dialog.is_none()
            && let Some((row, col)) = notes.cursor()
        {
            set_pty_cursor(frame, area, row, col);
        }
    }

    let footer = Paragraph::new(format!(
        "{}  ·  Alt+N новый чат · Alt+M заметка · Alt+стрелки панели · Alt+X закрыть · Alt+Q выход",
        app.status
    ));
    frame.render_widget(footer, areas[1]);

    if app.help_visible {
        let popup = centered_rect(86, 92, frame.area());
        let help_lines = vec![
            Line::from("Alt+←/→/↑/↓ переход между панелями · мышь: клик и колесо"),
            Line::from("Alt+M/Ь открыть/скрыть заметку · Alt+↓ фокус nvim"),
            Line::from("В nvim :w сохранить · :wq закрыть · ✎ в списке означает заметку"),
            Line::from("Скрытие сохраняет буфер · при выходе nvim спросит о сохранении"),
            Line::from("↑/↓ в сессиях выбрать чат · Enter открыть диалог"),
            Line::from("Alt+X/Ч закрыть только активный чат · Alt+Q/Й выйти из TUI"),
            Line::from("↑/↓ выбрать · ←/→ свернуть или развернуть проект"),
            Line::from("f или / поиск · r переименовать чат"),
            Line::from("p закрепить чат · x скрыть проект · m model override"),
            Line::from("Enter раскрыть папку / открыть выбранный чат"),
            Line::from("o возобновить · n / Alt+N/Т новый чат: выбор папки"),
            Line::from("В окне нового чата: ↑/↓ выбор · Enter запуск · Tab браузер"),
            Line::from("В браузере: Enter/→ вниз · ←/Backspace вверх · Space выбрать"),
            Line::from("q выйти из списка · Tab из списка в диалог"),
            Line::from(""),
            Line::from("Esc / ? закрыть · ↑/↓ прокрутка при узком окне"),
        ];
        app.help_scroll_limit = help_scroll_limit(popup, &help_lines);
        app.help_scroll = app.help_scroll.min(app.help_scroll_limit);
        let help = Paragraph::new(help_lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .scroll((app.help_scroll, 0))
            .block(Block::bordered().title("Справка по клавишам"));
        frame.render_widget(Clear, popup);
        frame.render_widget(help, popup);
    }
    let picker = app
        .new_chat_dialog
        .as_ref()
        .map(|dialog| render_new_chat(frame, dialog, theme));
    MouseLayout {
        projects: left_areas[0],
        sessions: left_areas[1],
        project_offset: list_state.offset(),
        session_offset: open_state.offset(),
        picker,
    }
}

fn set_pty_cursor(frame: &mut Frame, area: Rect, row: u16, col: u16) {
    if row < area.height.saturating_sub(2) && col < area.width.saturating_sub(2) {
        frame.set_cursor_position((
            area.x.saturating_add(1 + col),
            area.y.saturating_add(1 + row),
        ));
    }
}

fn render_new_chat(frame: &mut Frame, dialog: &NewChatDialog, theme: Theme) -> (Rect, usize) {
    let popup = centered_rect(86, 86, frame.area());
    let block = Block::bordered()
        .title(if dialog.browser.is_some() {
            "Новый чат · выбор папки"
        } else {
            "Новый чат · проекты"
        })
        .border_style(theme.focused_border(true))
        .title_style(theme.panel_title());
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(inner);
    let (heading, items, selected, hints) = if let Some(browser) = &dialog.browser {
        let mut items = vec![ListItem::new("✓  Использовать эту папку и начать чат")];
        if browser.has_parent() {
            items.push(ListItem::new("  .. (на уровень выше)"));
        }
        items.extend(browser.children.iter().map(|path| {
            let name = path.file_name().map(Path::new).unwrap_or(path);
            ListItem::new(Line::from(Span::styled(
                format!("  {}", display_path(name)),
                theme.project_heading(),
            )))
        }));
        (
            display_path(&browser.directory),
            items,
            browser.selected,
            "↑/↓ выбор · Enter/→ открыть · ←/Backspace вверх · Space начать чат · Esc назад",
        )
    } else {
        let mut items: Vec<_> = dialog
            .projects
            .iter()
            .map(|path| {
                let name = path.file_name().map(Path::new).unwrap_or(path);
                ListItem::new(Line::from(Span::styled(
                    format!("  {} · {}", display_path(name), display_path(path)),
                    theme.project_heading(),
                )))
            })
            .collect();
        items.push(ListItem::new("+  Выбрать другую папку…"));
        (
            dialog
                .projects
                .get(dialog.selected)
                .map(|path| format!("Папка нового чата: {}", display_path(path)))
                .unwrap_or_else(|| "Выберите другую папку через браузер каталогов".to_owned()),
            items,
            dialog.selected,
            "↑/↓ выбор · Enter начать чат / другая папка · Tab/→ браузер · Esc отмена",
        )
    };
    frame.render_widget(Paragraph::new(heading).wrap(Wrap { trim: false }), areas[0]);
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected_row()),
        areas[1],
        &mut state,
    );
    let mut footer = vec![Line::from(hints)];
    if let Some(error) = &dialog.error {
        footer.push(Line::from(Span::styled(
            error.clone(),
            theme.stopped_icon(),
        )));
    }
    frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: false }), areas[2]);
    (areas[1], state.offset())
}

fn help_scroll_limit(popup: Rect, lines: &[Line<'_>]) -> u16 {
    let width = usize::from(popup.width.saturating_sub(2)).max(1);
    let content_height = popup.height.saturating_sub(2);
    let rendered_height: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(width).max(1))
        .sum();
    rendered_height
        .saturating_sub(usize::from(content_height))
        .min(usize::from(u16::MAX)) as u16
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or("Проект"))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_chat::DirectoryBrowser;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use std::path::PathBuf;

    #[test]
    fn note_split_is_below_chat_and_preserves_dialog_bounds() {
        for (width, height) in [(120, 40), (40, 12), (10, 3)] {
            let bounds = Rect::new(0, 0, width, height);
            let (original, hidden) = dialog_areas(bounds, false);
            assert!(hidden.is_none());
            let (chat, note) = dialog_areas(bounds, true);
            let note = note.unwrap();
            assert_eq!((chat.x, chat.width), (original.x, original.width));
            assert_eq!((note.x, note.width), (original.x, original.width));
            assert_eq!(chat.bottom(), note.y);
            assert_eq!(note.bottom(), original.bottom());
            assert_eq!(chat.height + note.height, original.height);
        }
    }

    #[test]
    fn new_chat_popup_scrolls_to_selected_project_and_handles_small_windows() {
        let projects: Vec<_> = (0..100)
            .map(|index| PathBuf::from(format!("/project-{index:03}")))
            .collect();
        let dialog = NewChatDialog::new(projects.clone(), projects.last().map(PathBuf::as_path));
        let theme = Theme::from_environment();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                render_new_chat(frame, &dialog, theme);
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("/project-099"));
        for (width, height) in [(1, 1), (20, 6), (40, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render_new_chat(frame, &dialog, theme);
                })
                .unwrap();
        }
    }

    #[test]
    fn browser_highlight_accounts_for_parent_row_in_root_and_regular_directory() {
        let theme = Theme::from_environment();
        for path in [Path::new("/"), Path::new("/tmp")] {
            let mut dialog = NewChatDialog::new([], None);
            dialog.browser = Some(DirectoryBrowser {
                directory: path.to_path_buf(),
                children: vec![path.join("CHILD")],
                selected: 1 + usize::from(path.parent().is_some()),
            });
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| {
                    render_new_chat(frame, &dialog, theme);
                })
                .unwrap();
            let cell = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .find(|cell| cell.symbol() == "C")
                .unwrap();
            if let Some(background) = theme.selected_row().bg {
                assert_eq!(cell.bg, background);
            } else {
                assert!(cell.modifier.contains(Modifier::REVERSED));
            }
        }
    }
}
