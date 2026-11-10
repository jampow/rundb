use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults().themes;
        themes
            .remove("base16-ocean.dark")
            .or_else(|| themes.into_values().next())
            .expect("nenhum tema de syntax highlight disponível")
    })
}

fn highlight_code(code: &str, lang: &str) -> Vec<Line<'static>> {
    let code = code.trim_end_matches('\n');
    let plain = Style::default().fg(Color::Green);
    let syntax = if lang.is_empty() {
        None
    } else {
        syntaxes().find_syntax_by_token(lang)
    };

    let Some(syntax) = syntax else {
        return code
            .lines()
            .map(|l| Line::from(Span::styled(format!("  {l}"), plain)))
            .collect();
    };

    let mut h = HighlightLines::new(syntax, theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let mut spans = vec![Span::raw("  ")];
        match h.highlight_line(line, syntaxes()) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let text = text.trim_end_matches(['\n', '\r']);
                    if text.is_empty() {
                        continue;
                    }
                    let c = style.foreground;
                    let mut s = Style::default().fg(Color::Rgb(c.r, c.g, c.b));
                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::BOLD)
                    {
                        s = s.add_modifier(Modifier::BOLD);
                    }
                    spans.push(Span::styled(text.to_string(), s));
                }
            }
            Err(_) => spans.push(Span::styled(
                line.trim_end_matches(['\n', '\r']).to_string(),
                plain,
            )),
        }
        out.push(Line::from(spans));
    }
    out
}

struct Renderer {
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    styles: Vec<Style>,
    lists: Vec<Option<u64>>,
    quote_depth: usize,
    code: Option<(String, String)>,
    links: Vec<String>,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, patch: Style) {
        let s = self.style().patch(patch);
        self.styles.push(s);
    }

    fn flush(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        let mut spans = Vec::new();
        for _ in 0..self.quote_depth {
            spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
        }
        spans.append(&mut self.cur);
        self.lines.push(Line::from(spans));
    }

    fn blank(&mut self) {
        if !matches!(self.lines.last(), Some(l) if l.spans.is_empty()) && !self.lines.is_empty() {
            self.lines.push(Line::default());
        }
    }

    fn text(&mut self, t: &str) {
        if let Some((_, buf)) = self.code.as_mut() {
            buf.push_str(t);
        } else {
            let style = self.style();
            self.cur.push(Span::styled(t.to_string(), style));
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    self.flush();
                    let color = match level {
                        HeadingLevel::H1 => Color::Magenta,
                        HeadingLevel::H2 => Color::Cyan,
                        _ => Color::Yellow,
                    };
                    self.push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
                    let prefix = format!("{} ", "#".repeat(level as usize));
                    self.text(&prefix);
                }
                Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
                Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
                Tag::Strikethrough => {
                    self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT))
                }
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.quote_depth += 1;
                    self.push_style(
                        Style::default()
                            .fg(Color::Gray)
                            .add_modifier(Modifier::ITALIC),
                    );
                }
                Tag::List(start) => {
                    self.flush();
                    self.lists.push(start);
                }
                Tag::Item => {
                    self.flush();
                    let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                    let marker = match self.lists.last_mut() {
                        Some(Some(n)) => {
                            let m = format!("{n}. ");
                            *n += 1;
                            m
                        }
                        _ => "• ".to_string(),
                    };
                    self.cur.push(Span::raw(indent));
                    self.cur
                        .push(Span::styled(marker, Style::default().fg(Color::Yellow)));
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l
                            .split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::Link { dest_url, .. } => {
                    self.links.push(dest_url.to_string());
                    self.push_style(
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    self.styles.pop();
                    self.flush();
                    self.blank();
                }
                TagEnd::Paragraph => {
                    self.flush();
                    if self.lists.is_empty() {
                        self.blank();
                    }
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                    self.styles.pop();
                }
                TagEnd::BlockQuote(_) => {
                    self.flush();
                    self.styles.pop();
                    self.quote_depth = self.quote_depth.saturating_sub(1);
                    if self.quote_depth == 0 {
                        self.blank();
                    }
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                    if self.lists.is_empty() {
                        self.blank();
                    }
                }
                TagEnd::Item => self.flush(),
                TagEnd::CodeBlock => {
                    if let Some((lang, code)) = self.code.take() {
                        self.lines.extend(highlight_code(&code, &lang));
                        self.blank();
                    }
                }
                TagEnd::Link => {
                    self.styles.pop();
                    if let Some(url) = self.links.pop() {
                        self.cur.push(Span::styled(
                            format!(" ({url})"),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
                _ => {}
            },
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let style = self
                    .style()
                    .patch(Style::default().fg(Color::Green).bg(Color::Rgb(40, 44, 52)));
                self.cur.push(Span::styled(format!(" {t} "), style));
            }
            Event::SoftBreak => self.cur.push(Span::raw(" ")),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )));
                self.blank();
            }
            _ => {}
        }
    }
}

/// Converte markdown em texto estilizado para o ratatui, com syntax highlight
/// nos blocos de código.
pub fn render(markdown: &str) -> Text<'static> {
    let mut r = Renderer {
        lines: Vec::new(),
        cur: Vec::new(),
        styles: Vec::new(),
        lists: Vec::new(),
        quote_depth: 0,
        code: None,
        links: Vec::new(),
    };

    let options = Options::ENABLE_STRIKETHROUGH;
    for ev in Parser::new_ext(markdown, options) {
        r.event(ev);
    }
    r.flush();
    while matches!(r.lines.last(), Some(l) if l.spans.is_empty()) {
        r.lines.pop();
    }
    Text::from(r.lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &Text) -> Vec<String> {
        text.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn renders_basic_markdown() {
        let t = render("# Oi\n\n- a\n- b\n\n1. x\n\n**forte** e `code`\n");
        let lines = plain(&t);
        assert!(lines.contains(&"# Oi".to_string()));
        assert!(lines.contains(&"• a".to_string()));
        assert!(lines.contains(&"• b".to_string()));
        assert!(lines.contains(&"1. x".to_string()));
        assert!(lines.iter().any(|l| l.contains("forte") && l.contains("code")));
    }

    #[test]
    fn highlights_sql_blocks() {
        let t = render("```sql\nSELECT 1\n```");
        let line = &t.lines[0];
        assert!(line.spans.len() > 2, "esperava spans coloridos");
        assert!(plain(&t)[0].contains("SELECT 1"));
    }

    #[test]
    fn unknown_language_is_plain() {
        let t = render("```nope\nfoo\n```");
        assert_eq!(plain(&t), vec!["  foo".to_string()]);
    }
}
