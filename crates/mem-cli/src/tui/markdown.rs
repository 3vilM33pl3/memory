use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::theme::Theme;

pub(crate) fn render_markdown_lines(input: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');

        if let Some(fence) = line.trim_start().strip_prefix("```") {
            in_code_block = !in_code_block;
            if !fence.trim().is_empty() && in_code_block {
                lines.push(Line::from(vec![
                    Span::styled("code ", Style::default().fg(Theme::ACCENT_STRONG)),
                    Span::styled(
                        fence.trim().to_string(),
                        Style::default()
                            .fg(Theme::ACCENT_STRONG)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else {
                lines.push(Line::from(""));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(vec![Span::styled(
                format!("  {line}"),
                Style::default()
                    .fg(Theme::TEXT)
                    .bg(Theme::PANEL_ALT)
                    .add_modifier(Modifier::BOLD),
            )]));
            continue;
        }

        if line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        if is_thematic_break(line) {
            lines.push(Line::from(Span::styled(
                "─".repeat(32),
                Style::default().fg(Theme::BORDER),
            )));
            continue;
        }

        if let Some((level, content)) = parse_heading(line) {
            lines.push(Line::from(render_inline_markdown(
                content,
                heading_style(level),
            )));
            continue;
        }

        if let Some((depth, content)) = parse_blockquote(line) {
            let mut spans = vec![Span::styled(
                format!("{} ", "│ ".repeat(depth.max(1))),
                Style::default().fg(Theme::ACCENT),
            )];
            spans.extend(render_inline_markdown(
                content,
                Style::default()
                    .fg(Theme::TEXT)
                    .add_modifier(Modifier::ITALIC),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        if let Some((indent, marker, content, checked)) = parse_list_item(line) {
            let mut spans = vec![Span::styled(
                " ".repeat(indent),
                Style::default().fg(Theme::TEXT),
            )];
            let marker_span = match checked {
                Some(true) => Span::styled(
                    "[x] ".to_string(),
                    Style::default()
                        .fg(Theme::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                ),
                Some(false) => Span::styled(
                    "[ ] ".to_string(),
                    Style::default()
                        .fg(Theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                ),
                None => Span::styled(
                    marker,
                    Style::default()
                        .fg(Theme::ACCENT_STRONG)
                        .add_modifier(Modifier::BOLD),
                ),
            };
            spans.push(marker_span);
            spans.extend(render_inline_markdown(
                content,
                Style::default().fg(Theme::TEXT),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        lines.push(Line::from(render_inline_markdown(
            line,
            Style::default().fg(Theme::TEXT),
        )));
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

fn heading_style(level: usize) -> Style {
    let color = match level {
        1 => Theme::ACCENT_STRONG,
        2 => Theme::ACCENT,
        _ => Theme::TITLE,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3
        && (trimmed.chars().all(|ch| ch == '-')
            || trimmed.chars().all(|ch| ch == '*')
            || trimmed.chars().all(|ch| ch == '_'))
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|&ch| ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let content = trimmed[hashes..].trim_start();
    (!content.is_empty()).then_some((hashes, content))
}

fn parse_blockquote(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let mut depth = 0usize;
    let mut rest = trimmed;
    while let Some(remainder) = rest.strip_prefix('>') {
        depth += 1;
        rest = remainder.trim_start();
    }
    (depth > 0).then_some((depth, rest))
}

fn parse_list_item(line: &str) -> Option<(usize, String, &str, Option<bool>)> {
    let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let trimmed = &line[indent..];
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            if let Some(content) = rest.strip_prefix("[ ] ") {
                return Some((indent, String::new(), content, Some(false)));
            }
            if let Some(content) = rest.strip_prefix("[x] ") {
                return Some((indent, String::new(), content, Some(true)));
            }
            if let Some(content) = rest.strip_prefix("[X] ") {
                return Some((indent, String::new(), content, Some(true)));
            }
            return Some((indent, "• ".to_string(), rest, None));
        }
    }
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        let number = &trimmed[..digits];
        let content = &trimmed[(digits + 2)..];
        return Some((indent, format!("{number}. "), content, None));
    }
    None
}

pub(crate) fn render_inline_markdown(input: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut chars = input.chars().peekable();
    let mut emphasis = false;
    let mut strong = false;
    let mut code = false;

    let flush = |spans: &mut Vec<Span<'static>>,
                 buffer: &mut String,
                 emphasis: bool,
                 strong: bool,
                 code: bool,
                 base_style: Style| {
        if buffer.is_empty() {
            return;
        }
        spans.push(Span::styled(
            std::mem::take(buffer),
            inline_markdown_style(base_style, emphasis, strong, code),
        ));
    };

    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut label = String::new();
            let mut temp = chars.clone();
            let mut found_close = false;
            for next in temp.by_ref() {
                if next == ']' {
                    found_close = true;
                    break;
                }
                label.push(next);
            }
            if found_close {
                let mut temp_after = temp.clone();
                if temp_after.next() == Some('(') {
                    let mut url = String::new();
                    let mut found_url_close = false;
                    for next in temp_after {
                        if next == ')' {
                            found_url_close = true;
                            break;
                        }
                        url.push(next);
                    }
                    if found_url_close {
                        flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
                        for _ in 0..(label.chars().count() + url.chars().count() + 3) {
                            let _ = chars.next();
                        }
                        spans.push(Span::styled(
                            format!("{label} ({url})"),
                            inline_markdown_style(base_style, emphasis, strong, code)
                                .fg(Theme::ACCENT),
                        ));
                        continue;
                    }
                }
            }
            buffer.push(ch);
            continue;
        }

        if ch == '`' {
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            code = !code;
            continue;
        }

        if (ch == '*' || ch == '_') && chars.peek() == Some(&ch) {
            let _ = chars.next();
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            strong = !strong;
            continue;
        }

        if ch == '*' || ch == '_' {
            flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
            emphasis = !emphasis;
            continue;
        }

        buffer.push(ch);
    }

    flush(&mut spans, &mut buffer, emphasis, strong, code, base_style);
    if spans.is_empty() {
        vec![Span::styled(String::new(), base_style)]
    } else {
        spans
    }
}

fn inline_markdown_style(base_style: Style, emphasis: bool, strong: bool, code: bool) -> Style {
    let mut style = base_style;
    if emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if code {
        style = style.bg(Theme::PANEL_ALT).fg(Theme::ACCENT_STRONG);
    }
    style
}
