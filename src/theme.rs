use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    colors_enabled: bool,
    bright_colors_enabled: bool,
    light_background: Option<bool>,
}

impl Theme {
    pub fn from_environment() -> Self {
        let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
        let color_term = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        let term_has_basic_color = ["xterm", "screen", "tmux", "rxvt", "ansi", "linux"]
            .iter()
            .any(|prefix| term.starts_with(prefix))
            || term.contains("color");
        let term_has_bright_color = term.contains("256color");
        let colorterm_has_color = matches!(color_term.as_str(), "truecolor" | "24bit");
        let colors_enabled = std::env::var_os("NO_COLOR").is_none()
            && term != "dumb"
            && (term_has_basic_color || !color_term.is_empty());
        let bright_colors_enabled =
            colors_enabled && (term_has_bright_color || colorterm_has_color);
        let light_background = std::env::var("COLORFGBG")
            .ok()
            .and_then(|value| value.rsplit(';').next()?.parse::<u8>().ok())
            .filter(|value| *value <= 15)
            .map(|value| value >= 7);

        Self {
            colors_enabled,
            bright_colors_enabled,
            light_background,
        }
    }

    pub fn project_heading(self) -> Style {
        let color = match (self.colors_enabled, self.light_background) {
            (true, Some(true)) => Color::Blue,
            (true, Some(false)) if self.bright_colors_enabled => Color::LightCyan,
            (true, Some(false)) | (true, None) => Color::Cyan,
            _ => Color::Reset,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn project_icon(self) -> Style {
        let color = match (self.colors_enabled, self.light_background) {
            (true, Some(true)) => Color::Yellow,
            (true, Some(false)) if self.bright_colors_enabled => Color::LightYellow,
            (true, Some(false)) | (true, None) => Color::Yellow,
            _ => Color::Reset,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn session_icon(self, active: bool) -> Style {
        let color = match (self.colors_enabled, self.light_background, active) {
            (true, Some(true), true) => Color::Blue,
            (true, Some(true), false) => Color::Green,
            (true, Some(false) | None, true) if self.bright_colors_enabled => Color::LightGreen,
            (true, Some(false) | None, true) => Color::Green,
            (true, Some(false) | None, false) if self.bright_colors_enabled => Color::LightCyan,
            (true, Some(false) | None, false) => Color::Cyan,
            _ => Color::Reset,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn history_icon(self) -> Style {
        let color = match (self.colors_enabled, self.light_background) {
            (true, Some(true)) => Color::DarkGray,
            (true, Some(false)) if self.bright_colors_enabled => Color::DarkGray,
            (true, Some(false) | None) if self.bright_colors_enabled => Color::Gray,
            (true, Some(false) | None) => Color::White,
            _ => Color::Reset,
        };
        Style::default().fg(color)
    }

    pub fn stopped_icon(self) -> Style {
        let color = if self.colors_enabled {
            Color::Red
        } else {
            Color::Reset
        };
        Style::default().fg(color)
    }

    pub fn active_chat(self) -> Style {
        self.project_heading()
    }

    pub fn panel_border(self) -> Style {
        let color = match (self.colors_enabled, self.light_background) {
            (true, Some(true)) if self.bright_colors_enabled => Color::DarkGray,
            (true, Some(true)) => Color::Black,
            (true, Some(false)) if self.bright_colors_enabled => Color::DarkGray,
            (true, Some(false)) | (true, None) => Color::Cyan,
            _ => Color::Reset,
        };
        Style::default().fg(color)
    }

    pub fn focused_border(self, focused: bool) -> Style {
        if !focused {
            return self.panel_border();
        }
        // ANSI palette slots are resolved by the terminal's current theme.
        let color = match (self.colors_enabled, self.bright_colors_enabled) {
            (true, true) => Color::LightMagenta,
            (true, false) => Color::Magenta,
            _ => Color::Reset,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn focused_title(self, focused: bool) -> Style {
        if focused {
            self.focused_border(true)
        } else {
            self.panel_title()
        }
    }

    pub fn panel_title(self) -> Style {
        self.project_heading()
    }

    pub fn selected_row(self) -> Style {
        if !self.colors_enabled {
            return Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        match self.light_background {
            Some(true) => Style::default().fg(Color::White).bg(Color::Blue),
            Some(false) | None if self.bright_colors_enabled => {
                Style::default().fg(Color::Black).bg(Color::LightCyan)
            }
            Some(false) | None => Style::default().fg(Color::Black).bg(Color::Cyan),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_panel_is_distinct_on_light_dark_and_unknown_backgrounds() {
        for light_background in [None, Some(true), Some(false)] {
            for bright_colors_enabled in [false, true] {
                let theme = Theme {
                    colors_enabled: true,
                    bright_colors_enabled,
                    light_background,
                };
                assert_ne!(
                    theme.focused_border(true).fg,
                    theme.focused_border(false).fg
                );
                assert!(theme.focused_title(true).bg.is_none());
                assert_ne!(theme.focused_title(true).fg, theme.focused_title(false).fg);
                assert!(theme.focused_title(false).bg.is_none());
            }
        }
        let monochrome = Theme {
            colors_enabled: false,
            bright_colors_enabled: false,
            light_background: None,
        };
        assert_eq!(monochrome.focused_border(true).fg, Some(Color::Reset));
        assert!(
            monochrome
                .focused_title(true)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}
