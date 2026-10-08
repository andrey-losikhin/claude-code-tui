mod app;
mod commands;
mod config;
mod events;
mod new_chat;
mod notes;
mod pty;
mod search;
mod sessions;
mod theme;
mod ui;
mod workspace;

use std::io;

use app::{AppAction, FocusPanel};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::event::{KeyCode, KeyModifiers};
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|arg| arg == "--tui-hook") {
        if let Some(path) = args.next() {
            events::emit_hook(std::path::Path::new(&path));
        }
        return Ok(());
    }
    let mut terminal = ratatui::try_init()?;
    let mut _terminal_guard = TerminalRestoreGuard { enhanced: false };
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        crossterm::execute!(
            io::stdout(),
            event::PushKeyboardEnhancementFlags(
                event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        )?;
        _terminal_guard.enhanced = true;
    }
    crossterm::execute!(
        io::stdout(),
        EnableMouseCapture,
        event::EnableBracketedPaste
    )?;
    let mut app = app::App::load();
    let theme = theme::Theme::from_environment();
    let mut processes = pty::PtyManager::default();
    let bridge = match events::EventBridge::new() {
        Ok(bridge) => Some(bridge),
        Err(error) => {
            app.status = format!("События CLI недоступны, статус неизвестен: {error}");
            None
        }
    };
    processes.hook_settings = bridge.as_ref().map(|bridge| bridge.settings.clone());
    let mut notes = notes::NotesManager::default();
    let mut output_viewer = pty::PtyManager::default();
    let mut output_snapshot: Option<OutputSnapshot> = None;
    let mut chat_selection_enabled = true;

    let search_worker = search::SearchWorker::new();
    let mut search_query_sent = String::new();
    let mut selected_fragment: Option<String> = None;
    let mut layout_drag: Option<bool> = None;
    let mut previous_chat: Option<String> = None;
    let mut tracked_chat: Option<String> = None;
    let notifier = events::Notifier::new();
    let mut last_history_refresh = Instant::now();
    while !app.should_quit {
        output_viewer.drain_output();
        output_viewer.remove_stopped();
        if output_viewer.active_id().is_none() {
            drop(output_snapshot.take());
        }
        let completed = processes.drain_output();
        if let Some(bridge) = &bridge {
            for event in bridge.drain() {
                if processes.is_running(&event.session_id) {
                    let visible = processes.active_id() == Some(event.session_id.as_str())
                        && app.focused_panel == FocusPanel::Dialogue
                        && !app.help_visible
                        && !app.input_active()
                        && app.popup.is_none()
                        && output_viewer.active_id().is_none();
                    let id = event.session_id.clone();
                    if app.activity.apply(event, visible) {
                        let (desktop, sound) = app.notification_settings();
                        notifier.notify(desktop, sound);
                        app.status = format!(
                            "{} · {}",
                            app.display_title_for_id(&id),
                            app.activity.label(&id)
                        );
                    }
                }
            }
        }
        if app.focused_panel == FocusPanel::Dialogue
            && !app.input_active()
            && !app.help_visible
            && app.popup.is_none()
            && output_viewer.active_id().is_none()
            && let Some(id) = processes.active_id()
        {
            app.activity.unread.remove(id);
        }
        let active_finished = processes
            .active_id()
            .is_some_and(|id| !processes.is_running(id));
        processes.remove_stopped();
        if active_finished && !notes.exit_requested {
            app.focus_projects();
            app.selected_session = None;
            app.status = "CLI завершён · чат убран из открытых".to_owned();
        }
        if completed
            || (!app.input_active() && last_history_refresh.elapsed() >= Duration::from_secs(2))
        {
            app.sync_live_sessions(&processes.open_sessions());
            last_history_refresh = Instant::now();
        }
        app.poll_history(&processes.open_sessions());
        processes.sync_titles(|id| app.title_for_id(id).map(str::to_owned));
        if notes.sync(processes.active_id()) {
            app.should_quit = true;
            continue;
        }
        if app.focused_panel == FocusPanel::Notes && !notes.is_visible() {
            app.focused_panel = if processes.active_id().is_some() {
                FocusPanel::Dialogue
            } else {
                FocusPanel::Projects
            };
        }
        if let Some(popup) = &mut app.popup
            && popup.kind == workspace::Kind::Search
        {
            if let Some(result) = search_worker.result() {
                popup.entries = result.entries;
                popup.selected = 0;
                popup.hint = format!(
                    "{} результатов · {} пропущено{}",
                    popup.entries.len(),
                    result.skipped,
                    if result.limited {
                        " · поиск ограничен"
                    } else {
                        ""
                    }
                );
            }
            if popup.query != search_query_sent {
                let query = popup.query.clone();
                let sources = search::sources(
                    &app.sessions,
                    |id| app.display_title_for_id(id),
                    notes.catalog(),
                );
                if search_worker.request(query.clone(), sources).is_some() {
                    search_query_sent = query;
                    if let Some(popup) = &mut app.popup {
                        popup.entries.clear();
                        popup.hint = "Поиск…".into();
                    }
                }
            }
        }
        let current_chat = processes.active_id().map(str::to_owned);
        if current_chat != tracked_chat {
            selected_fragment = None;
            if tracked_chat.is_some() {
                previous_chat = tracked_chat.take();
            }
            tracked_chat = current_chat;
        }
        let terminal_size = terminal.size()?;
        let bounds = ratatui::layout::Rect::new(0, 0, terminal_size.width, terminal_size.height);
        output_viewer.resize(bounds);
        let (chat_area, note_area) = ui::configured_dialog_areas(
            bounds,
            notes.is_visible() && !app.maximized,
            &app.layout(),
        );
        processes.resize(chat_area);
        if let Some(area) = note_area {
            notes.resize(area);
        }
        let chat_lines = processes.active_lines();
        let open_sessions = processes.open_sessions();
        let cursor = processes.active_cursor();
        let mut mouse_layout = ui::MouseLayout::default();
        terminal.draw(|frame| {
            if output_viewer.active_id().is_some() {
                let block = ratatui::widgets::Block::bordered().title(
                    "Вывод активного чата · :q / Alt+B вернуться · мышью выделить и скопировать",
                );
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(output_viewer.active_lines()).block(block),
                    bounds,
                );
                if let Some((row, col)) = output_viewer.active_cursor() {
                    frame.set_cursor_position((bounds.x + 1 + col, bounds.y + 1 + row));
                }
                return;
            }
            mouse_layout = ui::render(
                frame,
                &mut app,
                theme,
                &chat_lines,
                &open_sessions,
                processes.active_title(),
                cursor,
                &notes,
            );
        })?;

        if event::poll(Duration::from_millis(16))? {
            let mut input = event::read()?;
            if let Event::Key(key) = &input
                && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            {
                processes.clear_selection();
                output_viewer.clear_selection();
            }
            if let Event::Key(key) = &input
                && let Some(panel) = commands::numbered_panel(key)
            {
                output_viewer.close_active()?;
                drop(output_snapshot.take());
                search_worker.cancel();
                focus_numbered_panel(panel, &mut app, &processes, &mut notes, bounds);
                continue;
            }
            if app.popup.is_some() {
                let chosen = match &input {
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        if key.code == KeyCode::Esc {
                            app.popup = None;
                            search_worker.cancel();
                            continue;
                        }
                        if key.modifiers == KeyModifiers::ALT {
                            app.popup = None;
                            Some(workspace::Target::Command(match key.code {
                                KeyCode::Char(c) => app::App::shortcut_character(c),
                                _ => '\0',
                            }))
                        } else {
                            app.popup.as_mut().and_then(|popup| popup.key(key.code))
                        }
                    }
                    Event::Mouse(mouse) => {
                        if let Some((rect, offset)) = mouse_layout.popup {
                            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                                && rect.contains((mouse.column, mouse.row).into())
                                && mouse.row > rect.y
                                && mouse.row < rect.bottom().saturating_sub(1)
                            {
                                if let Some(popup) = &mut app.popup {
                                    popup.selected = offset + usize::from(mouse.row - rect.y - 1);
                                    popup.key(KeyCode::Enter)
                                } else {
                                    None
                                }
                            } else if let Some(popup) = &mut app.popup {
                                match mouse.kind {
                                    MouseEventKind::ScrollDown => popup.key(KeyCode::Down),
                                    MouseEventKind::ScrollUp => popup.key(KeyCode::Up),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    Event::Paste(text) => {
                        if let Some(popup) = &mut app.popup {
                            for character in text.chars().filter(|c| !c.is_control()) {
                                popup.key(KeyCode::Char(character));
                            }
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(target) = chosen {
                    app.popup = None;
                    match target {
                        workspace::Target::Command(character) => {
                            if character == '\0' { /* Keep the original Alt arrow event. */
                            } else {
                                input = Event::Key(crossterm::event::KeyEvent::new(
                                    KeyCode::Char(character),
                                    KeyModifiers::ALT,
                                ));
                            }
                        }
                        workspace::Target::Chat(id, project) => {
                            let action = app.resume_id(id, project);
                            start_cli_action(action, &mut processes, &mut app, chat_area)?;
                            continue;
                        }
                        workspace::Target::Note(id, path) => {
                            app.maximized = false;
                            if let Some(session) =
                                app.sessions.iter().find(|session| session.id == id)
                            {
                                let project = session.project_path.clone();
                                let action = app.resume_id(id.clone(), project.clone());
                                start_cli_action(action, &mut processes, &mut app, chat_area)?;
                                if processes.active_id() == Some(id.as_str()) {
                                    match notes.open(
                                        &id,
                                        &project,
                                        ui::configured_dialog_areas(bounds, true, &app.layout())
                                            .1
                                            .unwrap_or(chat_area),
                                    ) {
                                        Ok(()) => app.focused_panel = FocusPanel::Notes,
                                        Err(error) => {
                                            app.status =
                                                format!("Не удалось открыть заметку: {error}")
                                        }
                                    }
                                }
                            } else {
                                if let Err(error) = output_viewer.start_output_viewer(
                                    &path,
                                    path.parent().unwrap_or(&path),
                                    bounds,
                                ) {
                                    app.status = format!("Не удалось открыть заметку: {error}");
                                }
                            }
                            continue;
                        }
                        workspace::Target::Search {
                            id,
                            project,
                            text,
                            note,
                            line,
                        } => {
                            if let Some(path) = note {
                                if let Err(error) = output_viewer.start_search_viewer(
                                    &path,
                                    path.parent().unwrap_or(&path),
                                    bounds,
                                    line,
                                ) {
                                    app.status = format!("Не удалось открыть результат: {error}");
                                }
                            } else {
                                let action = app.resume_id(id, project.clone());
                                start_cli_action(action, &mut processes, &mut app, chat_area)?;
                                match OutputSnapshot::create(&text).and_then(|snapshot| {
                                    output_viewer.start_search_viewer(
                                        &snapshot.path,
                                        &project,
                                        bounds,
                                        line,
                                    )?;
                                    Ok(snapshot)
                                }) {
                                    Ok(snapshot) => output_snapshot = Some(snapshot),
                                    Err(error) => {
                                        app.status =
                                            format!("Не удалось открыть результат: {error}")
                                    }
                                }
                            }
                            continue;
                        }
                    }
                } else {
                    continue;
                }
            }
            if let Event::Key(key) = &input
                && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                && key.modifiers == KeyModifiers::ALT
            {
                let shortcut = match key.code {
                    KeyCode::Char(character) => app::App::shortcut_character(character),
                    _ => '\0',
                };
                if output_viewer.active_id().is_some()
                    && (commands::COMMANDS
                        .iter()
                        .any(|command| command.key == shortcut)
                        && !matches!(shortcut, 'b' | 'c')
                        || matches!(
                            key.code,
                            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                        ))
                {
                    output_viewer.close_active()?;
                    drop(output_snapshot.take());
                }
                if let Some(panel) = commands::numbered_panel(key) {
                    search_worker.cancel();
                    focus_numbered_panel(panel, &mut app, &processes, &mut notes, bounds);
                    continue;
                }
                match shortcut {
                    'u' => {
                        app.toggle_sidebar();
                        continue;
                    }
                    'd' => {
                        app.maximized = !app.maximized;
                        app.focused_panel = FocusPanel::Dialogue;
                        continue;
                    }
                    'h' => {
                        app.cancel_input_modes();
                        app.help_visible = !app.help_visible;
                        app.help_scroll = 0;
                        continue;
                    }
                    'z' => {
                        app.collapse_all();
                        continue;
                    }
                    't' | 'a' => {
                        app.toggle_notification(shortcut == 't');
                        continue;
                    }
                    'j' => {
                        if let Some(id) = &previous_chat
                            && let Some(session) =
                                app.sessions.iter().find(|session| &session.id == id)
                        {
                            let action = app.resume_id(id.clone(), session.project_path.clone());
                            start_cli_action(action, &mut processes, &mut app, chat_area)?;
                        }
                        continue;
                    }
                    'f' | 'l' => {
                        app.cancel_input_modes();
                        app.help_visible = false;
                        search_worker.cancel();
                        search_query_sent.clear();
                        let entries = if shortcut == 'l' {
                            notes
                                .catalog()
                                .into_iter()
                                .map(|(id, path)| workspace::Entry {
                                    label: format!(
                                        "✎ {} · {}",
                                        app.display_title_for_id(&id),
                                        path.display()
                                    ),
                                    target: workspace::Target::Note(id, path),
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        app.popup = Some(workspace::Popup::new(
                            if shortcut == 'l' {
                                workspace::Kind::Notes
                            } else {
                                workspace::Kind::Search
                            },
                            entries,
                        ));
                        continue;
                    }
                    'e' => {
                        app.maximized = false;
                        if let (Some(text), Some(id), Some(project)) = (
                            &selected_fragment,
                            processes.active_id(),
                            processes.active_project_path(),
                        ) {
                            match notes.append_selection(
                                id,
                                project,
                                ui::configured_dialog_areas(bounds, true, &app.layout())
                                    .1
                                    .unwrap_or(chat_area),
                                text,
                            ) {
                                Ok(()) => {
                                    app.focused_panel = FocusPanel::Notes;
                                    app.status =
                                        "Фрагмент добавлен в заметку · :w сохранить".into();
                                }
                                Err(error) => {
                                    app.status = format!("Не удалось добавить фрагмент: {error}")
                                }
                            }
                        } else {
                            app.status = "Сначала выделите текст мышью в открытом чате".into();
                        }
                        continue;
                    }
                    's' | 'k' => {
                        app.cancel_input_modes();
                        app.help_visible = false;
                        let entries = if shortcut == 'k' {
                            commands::COMMANDS
                                .iter()
                                .map(|command| workspace::Entry {
                                    label: format!("{} · {}", command.label, command.shortcut()),
                                    target: workspace::Target::Command(command.key),
                                })
                                .collect()
                        } else {
                            let mut entries = Vec::new();
                            for session in &open_sessions {
                                entries.push(workspace::Entry {
                                    label: format!(
                                        "◉ {} · {}",
                                        session.title,
                                        session.project_path.display()
                                    ),
                                    target: workspace::Target::Chat(
                                        session.id.clone(),
                                        session.project_path.clone(),
                                    ),
                                });
                            }
                            for session in &app.sessions {
                                if !open_sessions.iter().any(|open| open.id == session.id) {
                                    entries.push(workspace::Entry {
                                        label: format!(
                                            "{} · {}",
                                            app.display_title_for_id(&session.id),
                                            session.project_path.display()
                                        ),
                                        target: workspace::Target::Chat(
                                            session.id.clone(),
                                            session.project_path.clone(),
                                        ),
                                    });
                                }
                            }
                            entries
                        };
                        app.popup = Some(workspace::Popup::new(
                            if shortcut == 'k' {
                                workspace::Kind::Commands
                            } else {
                                workspace::Kind::Switcher
                            },
                            entries,
                        ));
                        continue;
                    }
                    _ => {}
                }
                if shortcut == 'c' {
                    chat_selection_enabled = !chat_selection_enabled;
                    app.status = if chat_selection_enabled {
                        "Мышь: выделение только чата · копирование при отпускании"
                    } else {
                        "Мышь передаётся CLI/nvim · Alt+C вернуть выделение чата"
                    }
                    .to_owned();
                    continue;
                }
                if matches!(shortcut, 'v' | 'b') && !app.input_active() {
                    if output_viewer.active_id().is_some() {
                        output_viewer.close_active()?;
                        drop(output_snapshot.take());
                    } else if let (Some(text), Some(project)) =
                        (processes.active_output(), processes.active_project_path())
                    {
                        match OutputSnapshot::create(&text).and_then(|snapshot| {
                            output_viewer.start_output_viewer(&snapshot.path, project, bounds)?;
                            Ok(snapshot)
                        }) {
                            Ok(snapshot) => output_snapshot = Some(snapshot),
                            Err(error) => {
                                app.status = format!("Не удалось открыть вывод в nvim: {error}")
                            }
                        }
                    } else {
                        app.status = "Сначала откройте чат для просмотра вывода".to_owned();
                    }
                    continue;
                }
                if output_viewer.active_id().is_some()
                    && (matches!(shortcut, 'q' | 'x' | 'm' | 'n')
                        || matches!(
                            key.code,
                            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                        ))
                {
                    output_viewer.close_active()?;
                    drop(output_snapshot.take());
                }
            }
            if output_viewer.active_id().is_some() {
                if let Event::Mouse(mouse) = &input
                    && chat_selection_enabled
                {
                    let (handled, text) = output_viewer.select_mouse(*mouse, bounds);
                    if handled {
                        if let Some(text) = text {
                            let _ = output_viewer.send_mouse(*mouse, bounds);
                            copy_selection(&text, &mut app);
                        }
                        continue;
                    }
                }
                let result = match input {
                    Event::Key(key)
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        output_viewer.send_key(key)
                    }
                    Event::Paste(text) => output_viewer.send_paste(&text),
                    Event::Mouse(mouse) => output_viewer.send_mouse(mouse, bounds),
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    app.status = format!("Ошибка просмотра вывода: {error}");
                }
                continue;
            }
            if let Event::Paste(text) = &input {
                if !app.help_visible && app.new_chat_dialog.is_none() {
                    if app.input_active() {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            let _ = app.handle_key(KeyCode::Char(character));
                        }
                    } else {
                        let result = match app.focused_panel {
                            FocusPanel::Dialogue => processes.send_paste(text),
                            FocusPanel::Notes => notes.send_paste(text),
                            _ => Ok(()),
                        };
                        if let Err(error) = result {
                            app.status = format!("Ошибка вставки: {error}");
                        }
                    }
                }
                continue;
            }
            if let Event::Mouse(mouse) = input {
                if !app.input_active() && !app.help_visible {
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                        if !app.layout().sidebar_hidden
                            && (mouse.column == chat_area.x
                                || mouse.column.saturating_add(1) == chat_area.x)
                        {
                            layout_drag = Some(true);
                            continue;
                        }
                        if note_area.is_some_and(|area| {
                            mouse.column >= area.x
                                && (mouse.row == area.y || mouse.row.saturating_add(1) == area.y)
                        }) {
                            layout_drag = Some(false);
                            continue;
                        }
                    }
                    if let Some(sidebar) = layout_drag
                        && matches!(
                            mouse.kind,
                            MouseEventKind::Drag(MouseButton::Left)
                                | MouseEventKind::Up(MouseButton::Left)
                        )
                    {
                        if sidebar {
                            app.resize_layout(
                                Some(
                                    ((u32::from(mouse.column) * 100)
                                        / u32::from(bounds.width.max(1)))
                                        as u16,
                                ),
                                None,
                            );
                        } else {
                            app.resize_layout(
                                None,
                                Some(
                                    ((u32::from(mouse.row) * 100)
                                        / u32::from(bounds.height.saturating_sub(1).max(1)))
                                        as u16,
                                ),
                            );
                        }
                        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                            layout_drag = None;
                            app.save_layout();
                        }
                        continue;
                    }
                }
                if chat_selection_enabled && !app.input_active() && !app.help_visible {
                    let (handled, text) = processes.select_mouse(mouse, chat_area);
                    if handled {
                        app.focused_panel = FocusPanel::Dialogue;
                        if let Some(text) = text {
                            let _ = processes.send_mouse(mouse, chat_area);
                            selected_fragment = Some(text.clone());
                            copy_selection(&text, &mut app);
                        }
                        continue;
                    }
                }
                handle_mouse(
                    mouse,
                    &mouse_layout,
                    chat_area,
                    note_area,
                    &mut app,
                    &mut processes,
                    &mut notes,
                )?;
                continue;
            }
            if let Event::Key(key) = input
                && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            {
                if key.modifiers == KeyModifiers::ALT
                    && matches!(
                        key.code,
                        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                    )
                    && !app.input_active()
                    && !app.help_visible
                {
                    notes.cancel_exit();
                    app.focused_panel = adjacent_panel(
                        app.focused_panel,
                        key.code,
                        notes.is_visible() && !app.maximized,
                    );
                    if matches!(
                        app.focused_panel,
                        FocusPanel::Projects | FocusPanel::OpenSessions
                    ) && app.layout().sidebar_hidden
                    {
                        app.show_sidebar();
                    }
                    continue;
                }
                if let KeyCode::Char(character) = key.code
                    && key.modifiers == KeyModifiers::ALT
                    && app::App::shortcut_character(character) == 'q'
                {
                    request_app_exit(&mut app, &mut notes);
                } else if matches!(key.code, KeyCode::Char(character) if app::App::shortcut_character(character) == 'x')
                    && key.modifiers == KeyModifiers::ALT
                {
                    app.cancel_input_modes();
                    app.help_visible = false;
                    notes.cancel_exit();
                    match processes.close_active() {
                        Ok(Some(_)) => {
                            app.sync_live_sessions(&processes.open_sessions());
                            app.selected_session = None;
                            app.focus_projects();
                            app.status = "Чат закрыт; сессию можно открыть снова".to_owned();
                        }
                        Ok(None) => {
                            app.focus_projects();
                            app.status = "Нет открытого чата".to_owned();
                        }
                        Err(error) => {
                            app.focus_projects();
                            app.status = format!("Не удалось закрыть чат: {error}");
                        }
                    }
                } else if matches!(key.code, KeyCode::Char(character) if app::App::shortcut_character(character) == 'm')
                    && key.modifiers == KeyModifiers::ALT
                    && !app.input_active()
                {
                    notes.cancel_exit();
                    app.help_visible = false;
                    app.maximized = false;
                    if let (Some(id), Some(project)) =
                        (processes.active_id(), processes.active_project_path())
                    {
                        let (_, area) = ui::configured_dialog_areas(bounds, true, &app.layout());
                        match notes.toggle(id, project, area.unwrap_or(chat_area)) {
                            Ok(true) => {
                                app.focused_panel = FocusPanel::Notes;
                                app.status =
                                    "Заметка открыта · :w сохранить · Alt+M скрыть · Alt+↑ чат"
                                        .to_owned();
                            }
                            Ok(false) => {
                                app.focused_panel = FocusPanel::Dialogue;
                                app.status =
                                    "Заметка скрыта; буфер nvim сохранён в памяти".to_owned();
                            }
                            Err(error) => {
                                app.status = format!("Не удалось открыть заметку: {error}")
                            }
                        }
                    } else {
                        app.status = "Сначала откройте чат для заметки".to_owned();
                    }
                } else if matches!(key.code, KeyCode::Char(character) if app::App::shortcut_character(character) == 'n')
                    && !app.input_active()
                    && (key.modifiers == KeyModifiers::ALT
                        || (app.focused_panel == FocusPanel::Projects
                            && !key.modifiers.contains(KeyModifiers::CONTROL)))
                {
                    notes.cancel_exit();
                    app.begin_new_chat(
                        processes.active_project_path(),
                        open_sessions
                            .iter()
                            .map(|session| session.project_path.clone())
                            .collect(),
                    );
                } else if app.new_chat_dialog.is_some() {
                    if let Some(action) = app.handle_key(key.code) {
                        start_cli_action(action, &mut processes, &mut app, chat_area)?;
                    }
                } else if app.help_visible {
                    let _ = app.handle_key(key.code);
                } else if app.focused_panel == FocusPanel::OpenSessions
                    && !app.input_active()
                    && matches!(key.code, KeyCode::Up | KeyCode::Down)
                {
                    if processes.cycle(key.code == KeyCode::Up) {
                        app.select_session_id(
                            processes.active_id(),
                            processes.active_project_path(),
                        );
                    } else {
                        app.status = "Нет открытых CLI-сессий".to_owned();
                    }
                } else if app.focused_panel == FocusPanel::OpenSessions
                    && !app.input_active()
                    && key.modifiers.is_empty()
                    && matches!(key.code, KeyCode::Char(character) if app::App::shortcut_character(character) == 'r')
                {
                    if processes.active_id().is_some() {
                        app.select_session_id(
                            processes.active_id(),
                            processes.active_project_path(),
                        );
                        app.focus_projects();
                        let _ = app.handle_key(KeyCode::Char('r'));
                    } else {
                        app.status = "Выберите открытый чат для переименования".to_owned();
                    }
                } else if app.focused_panel == FocusPanel::OpenSessions
                    && key.code == KeyCode::Enter
                    && !app.input_active()
                {
                    if processes.active_id().is_some() {
                        app.focused_panel = FocusPanel::Dialogue;
                    }
                } else if app.focused_panel == FocusPanel::OpenSessions {
                    // Only navigation keys are meaningful in this panel.
                } else if key.code == KeyCode::Tab
                    && app.focused_panel == FocusPanel::Projects
                    && !app.input_active()
                {
                    if processes.active_id().is_some() {
                        app.focused_panel = FocusPanel::Dialogue;
                    } else {
                        app.status = "Сначала откройте чат клавишей Enter".to_owned();
                    }
                } else if app.focused_panel == FocusPanel::Dialogue {
                    if let Err(error) = processes.send_key(key) {
                        app.status = format!("Ошибка ввода CLI: {error}");
                    }
                } else if app.focused_panel == FocusPanel::Notes {
                    if let Err(error) = notes.send_key(key) {
                        app.status = format!("Ошибка редактора заметки: {error}");
                    }
                } else if let Some(action) = app.handle_key(key.code) {
                    start_cli_action(action, &mut processes, &mut app, chat_area)?;
                }
                if app.should_quit {
                    request_app_exit(&mut app, &mut notes);
                }
            }
        }
    }

    Ok(())
}

fn focus_numbered_panel(
    panel: FocusPanel,
    app: &mut app::App,
    processes: &pty::PtyManager,
    notes: &mut notes::NotesManager,
    bounds: ratatui::layout::Rect,
) {
    app.cancel_input_modes();
    app.popup = None;
    app.help_visible = false;
    notes.cancel_exit();
    match panel {
        FocusPanel::Projects => app.focus_projects(),
        FocusPanel::OpenSessions => {
            app.show_sidebar();
            app.focused_panel = panel;
        }
        FocusPanel::Dialogue => app.focused_panel = panel,
        FocusPanel::Notes => {
            if let (Some(id), Some(project)) =
                (processes.active_id(), processes.active_project_path())
            {
                app.maximized = false;
                let area = ui::configured_dialog_areas(bounds, true, &app.layout())
                    .1
                    .unwrap_or(bounds);
                match notes.open(id, project, area) {
                    Ok(()) => app.focused_panel = panel,
                    Err(error) => app.status = format!("Не удалось открыть заметку: {error}"),
                }
            } else {
                app.status = "Alt+4: сначала откройте чат для заметки".into();
            }
        }
    }
}

fn adjacent_panel(panel: FocusPanel, key: KeyCode, note: bool) -> FocusPanel {
    use FocusPanel::*;
    match (panel, key) {
        (Dialogue, KeyCode::Left) => Projects,
        (Notes, KeyCode::Left) => OpenSessions,
        (Projects, KeyCode::Right) => Dialogue,
        (OpenSessions, KeyCode::Right) if note => Notes,
        (OpenSessions, KeyCode::Right) => Dialogue,
        (Projects, KeyCode::Down) => OpenSessions,
        (OpenSessions, KeyCode::Up) => Projects,
        (Dialogue, KeyCode::Down) if note => Notes,
        (Notes, KeyCode::Up) => Dialogue,
        _ => panel,
    }
}

fn list_row(mouse: MouseEvent, area: ratatui::layout::Rect, offset: usize) -> Option<usize> {
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    inner
        .contains((mouse.column, mouse.row).into())
        .then(|| offset + usize::from(mouse.row - inner.y))
}

fn handle_mouse(
    mouse: MouseEvent,
    layout: &ui::MouseLayout,
    chat: ratatui::layout::Rect,
    note: Option<ratatui::layout::Rect>,
    app: &mut app::App,
    processes: &mut pty::PtyManager,
    notes: &mut notes::NotesManager,
) -> io::Result<()> {
    let click = mouse.kind == MouseEventKind::Down(MouseButton::Left);
    let scroll = match mouse.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if app.help_visible {
        if let Some(key) = scroll {
            let _ = app.handle_key(key);
        }
        return Ok(());
    }
    if app.new_chat_dialog.is_some() {
        if let Some((area, offset)) = layout.picker {
            if click {
                if let Some(row) = area
                    .contains((mouse.column, mouse.row).into())
                    .then(|| offset + usize::from(mouse.row - area.y))
                {
                    let dialog = app.new_chat_dialog.as_mut().unwrap();
                    let count = dialog
                        .browser
                        .as_ref()
                        .map(|browser| {
                            1 + usize::from(browser.has_parent()) + browser.children.len()
                        })
                        .unwrap_or(dialog.projects.len() + 1);
                    if row < count {
                        if let Some(browser) = &mut dialog.browser {
                            browser.selected = row;
                        } else {
                            dialog.selected = row;
                        }
                        if let Some(action) = app.handle_key(KeyCode::Enter) {
                            start_cli_action(action, processes, app, chat)?;
                        }
                    }
                }
            } else if area.contains((mouse.column, mouse.row).into())
                && let Some(key) = scroll
            {
                let _ = app.handle_key(key);
            }
        }
        return Ok(());
    }
    if app.input_active() {
        return Ok(());
    }
    let point = (mouse.column, mouse.row).into();
    if layout.projects.contains(point) {
        if click {
            app.focus_projects();
            if let Some(row) = list_row(mouse, layout.projects, layout.project_offset)
                .filter(|row| *row < app.rows.len())
            {
                app.selected_row = Some(row);
                if let Some(action) = app.handle_key(KeyCode::Enter) {
                    start_cli_action(action, processes, app, chat)?;
                }
            }
        } else if let Some(key) = scroll {
            app.focus_projects();
            let _ = app.handle_key(key);
        }
    } else if layout.sessions.contains(point) {
        if click {
            app.focused_panel = FocusPanel::OpenSessions;
            if let Some(row) = list_row(mouse, layout.sessions, layout.session_offset)
                && let Some(session) = processes.open_sessions().get(row)
            {
                processes.select_active(Some(&session.id));
                app.select_session_id(processes.active_id(), processes.active_project_path());
            }
        } else if let Some(key) = scroll {
            processes.cycle(key == KeyCode::Up);
            app.select_session_id(processes.active_id(), processes.active_project_path());
        }
    } else if chat.contains(point) {
        if click {
            app.focused_panel = FocusPanel::Dialogue;
            notes.cancel_exit();
        }
        if let Err(error) = processes.send_mouse(mouse, chat) {
            app.status = format!("Ошибка мыши CLI: {error}");
        }
    } else if let Some(area) = note.filter(|area| area.contains(point)) {
        if click {
            app.focused_panel = FocusPanel::Notes;
        }
        if let Err(error) = notes.send_mouse(mouse, area) {
            app.status = format!("Ошибка мыши nvim: {error}");
        }
    }
    Ok(())
}

fn request_app_exit(app: &mut app::App, notes: &mut notes::NotesManager) {
    app.cancel_input_modes();
    app.help_visible = false;
    match notes.request_exit() {
        Ok(ready) => {
            app.should_quit = ready;
            if !ready {
                app.maximized = false;
                app.focused_panel = FocusPanel::Notes;
                app.status =
                    "Выход: подтвердите сохранение в nvim · Alt+↑ отменить выход".to_owned();
            }
        }
        Err(error) => {
            app.should_quit = false;
            app.focused_panel = FocusPanel::Notes;
            app.status = format!("Не удалось завершить nvim: {error}");
        }
    }
}

struct TerminalRestoreGuard {
    enhanced: bool,
}

struct OutputSnapshot {
    path: std::path::PathBuf,
}

fn copy_selection(text: &str, app: &mut app::App) {
    if text.is_empty() {
        return;
    }
    app.status = match write_clipboard(text) {
        Ok(true) => "Выделенный текст чата скопирован".to_owned(),
        Ok(false) => "Текст чата отправлен в буфер обмена терминала (OSC 52)".to_owned(),
        Err(error) => format!("Не удалось скопировать текст: {error}"),
    };
}

fn write_clipboard(text: &str) -> io::Result<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for (program, args) in [
        ("wl-copy", vec!["--type", "text/plain;charset=utf-8"]),
        ("xclip", vec!["-selection", "clipboard", "-in"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let result = child.stdin.take().unwrap().write_all(text.as_bytes());
        if child.wait().is_ok_and(|status| status.success()) && result.is_ok() {
            return Ok(true);
        }
    }
    let encoded = clipboard_base64(text.as_bytes());
    let mut stdout = io::stdout().lock();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()?;
    Ok(false)
}

fn clipboard_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        result.push(ALPHABET[((bits >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((bits >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((bits >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(bits & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

impl OutputSnapshot {
    fn create(text: &str) -> io::Result<Self> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!(
            "claude-code-tui-output-{}.txt",
            uuid::Uuid::new_v4()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let snapshot = Self { path };
        file.write_all(text.as_bytes())?;
        Ok(snapshot)
    }
}

impl Drop for OutputSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = crossterm::execute!(io::stdout(), event::PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::execute!(
            io::stdout(),
            DisableMouseCapture,
            event::DisableBracketedPaste
        );
        let _ = ratatui::try_restore();
    }
}

fn start_cli_action(
    action: AppAction,
    processes: &mut pty::PtyManager,
    app: &mut app::App,
    area: ratatui::layout::Rect,
) -> io::Result<()> {
    let new_project = match &action {
        AppAction::NewChat { project_path, .. } => Some(project_path.clone()),
        _ => None,
    };
    let result = match action {
        AppAction::Resume {
            id,
            project_path,
            model,
        } => processes.start(
            Some(&id),
            app.display_title_for_id(&id),
            &project_path,
            Some(&id),
            model.as_deref(),
            area,
        ),
        AppAction::NewChat {
            project_path,
            model,
        } => {
            let title = project_path
                .file_name()
                .map(std::path::Path::new)
                .map(new_chat::display_path)
                .unwrap_or_else(|| "Новый чат".to_owned());
            processes.start(
                None,
                format!("Новый чат · {title}"),
                &project_path,
                None,
                model.as_deref(),
                area,
            )
        }
    };
    match result {
        Ok(id) => {
            app.sync_live_sessions(&processes.open_sessions());
            let config_warning = new_project.and_then(|path| app.finish_new_chat(path));
            app.select_session_id(Some(&id), processes.active_project_path());
            app.focused_panel = FocusPanel::Dialogue;
            app.status = "CLI-сессия открыта · Alt+стрелки панели · Alt+X закрыть".to_owned();
            if let Some(warning) = config_warning {
                app.status.push_str(&format!(" · {warning}"));
            }
        }
        Err(error) => {
            app.status = format!("Не удалось открыть Claude Code: {error}");
            if let Some(dialog) = &mut app.new_chat_dialog {
                dialog.error = Some(app.status.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn clipboard_encoding_is_valid_for_padding_and_unicode() {
        assert_eq!(clipboard_base64(b""), "");
        assert_eq!(clipboard_base64(b"f"), "Zg==");
        assert_eq!(clipboard_base64(b"fo"), "Zm8=");
        assert_eq!(clipboard_base64(b"foo"), "Zm9v");
        assert_eq!(clipboard_base64("я\n".as_bytes()), "0Y8K");
    }

    #[test]
    fn output_snapshot_is_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let snapshot = OutputSnapshot::create("output\n").unwrap();
        let path = snapshot.path.clone();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "output\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(snapshot);
        assert!(!path.exists());
    }
    #[test]
    fn directional_navigation_and_scrolled_rows() {
        assert_eq!(
            adjacent_panel(FocusPanel::Dialogue, KeyCode::Left, false),
            FocusPanel::Projects
        );
        assert_eq!(
            adjacent_panel(FocusPanel::Projects, KeyCode::Down, false),
            FocusPanel::OpenSessions
        );
        assert_eq!(
            adjacent_panel(FocusPanel::OpenSessions, KeyCode::Right, true),
            FocusPanel::Notes
        );
        assert_eq!(
            adjacent_panel(FocusPanel::Notes, KeyCode::Up, true),
            FocusPanel::Dialogue
        );
        assert_eq!(
            adjacent_panel(FocusPanel::Dialogue, KeyCode::Down, false),
            FocusPanel::Dialogue
        );
        let area = ratatui::layout::Rect::new(0, 0, 40, 10);
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(list_row(mouse, area, 7), Some(9));
        assert_eq!(list_row(MouseEvent { column: 0, ..mouse }, area, 7), None);
    }
}
