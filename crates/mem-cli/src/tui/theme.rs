use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders},
};

pub(crate) struct Theme;

impl Theme {
    pub(crate) const BACKGROUND: Color = Color::Rgb(12, 18, 28);
    pub(crate) const PANEL: Color = Color::Rgb(22, 31, 46);
    pub(crate) const PANEL_ALT: Color = Color::Rgb(28, 39, 58);
    pub(crate) const BORDER: Color = Color::Rgb(74, 94, 122);
    pub(crate) const TITLE: Color = Color::Rgb(146, 195, 255);
    pub(crate) const TEXT: Color = Color::Rgb(230, 236, 245);
    pub(crate) const MUTED: Color = Color::Rgb(150, 165, 186);
    pub(crate) const ACCENT: Color = Color::Rgb(92, 194, 255);
    pub(crate) const ACCENT_STRONG: Color = Color::Rgb(255, 196, 85);
    pub(crate) const SUCCESS: Color = Color::Rgb(104, 211, 145);
    pub(crate) const WARNING: Color = Color::Rgb(255, 187, 92);
    pub(crate) const DANGER: Color = Color::Rgb(255, 122, 122);
    pub(crate) const SELECTION_BG: Color = Color::Rgb(61, 96, 153);
    pub(crate) const SELECTION_FG: Color = Color::Rgb(250, 251, 255);
}

pub(crate) fn themed_block<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(title)
        .title_style(
            Style::default()
                .fg(Theme::TITLE)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Theme::PANEL))
}

pub(crate) fn themed_focus_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    let border = if focused {
        Theme::ACCENT
    } else {
        Theme::BORDER
    };
    let title_color = if focused {
        Theme::ACCENT_STRONG
    } else {
        Theme::TITLE
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border).add_modifier(if focused {
            Modifier::BOLD
        } else {
            Modifier::empty()
        }))
        .title(title)
        .title_style(
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Theme::PANEL))
}
