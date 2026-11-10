use crate::db_script::ScriptEntry;
use crate::markdown;
use anyhow::{bail, Result};
use dialoguer::Select;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::collections::HashMap;
use std::io::IsTerminal;

struct App<'a> {
    entries: &'a [ScriptEntry],
    filter: String,
    filtering: bool,
    pos: usize,
    scroll: u16,
    cache: HashMap<usize, Text<'static>>,
}

impl<'a> App<'a> {
    fn visible(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.file.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    fn current(&self) -> Option<usize> {
        self.visible().get(self.pos).copied()
    }

    fn description(&mut self) -> Text<'static> {
        let Some(idx) = self.current() else {
            return Text::from("Nenhum script corresponde ao filtro.");
        };
        let entries = self.entries;
        self.cache
            .entry(idx)
            .or_insert_with(|| {
                let entry = &entries[idx];
                if let Some(err) = &entry.error {
                    let mut text = Text::from(Line::styled(
                        "Erro ao carregar o script:",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ));
                    text.extend(Text::from(err.clone()));
                    text
                } else if let Some(desc) = &entry.description {
                    markdown::render(desc)
                } else {
                    Text::from(Line::styled(
                        "(sem descrição)",
                        Style::default().fg(Color::DarkGray),
                    ))
                }
            })
            .clone()
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            return;
        }
        self.pos = (self.pos as isize + delta).clamp(0, len as isize - 1) as usize;
        self.scroll = 0;
    }

    fn reset_selection(&mut self) {
        self.pos = 0;
        self.scroll = 0;
    }
}

fn draw(f: &mut Frame, app: &App, description: Text<'static>) {
    let [main, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(main);

    let visible = app.visible();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let e = &app.entries[i];
            if e.error.is_some() {
                ListItem::new(format!("⚠ {}", e.file)).style(Style::default().fg(Color::Red))
            } else {
                ListItem::new(e.file.clone())
            }
        })
        .collect();

    let title = if app.filtering || !app.filter.is_empty() {
        format!(" Scripts  /{} ", app.filter)
    } else {
        " Scripts ".to_string()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = ListState::default();
    if !visible.is_empty() {
        state.select(Some(app.pos));
    }
    f.render_stateful_widget(list, left, &mut state);

    let name = app
        .current()
        .map(|i| app.entries[i].file.as_str())
        .unwrap_or("");
    let paragraph = Paragraph::new(description)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {name} ")),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(paragraph, right);

    let help = if app.filtering {
        "Digite para filtrar · Enter confirma · Esc limpa"
    } else {
        "↑/↓ navega · Enter executa · / filtra · PgUp/PgDn rola descrição · Esc sai"
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        footer,
    );
}

fn run(terminal: &mut DefaultTerminal, entries: &[ScriptEntry]) -> Result<Option<String>> {
    let mut app = App {
        entries,
        filter: String::new(),
        filtering: false,
        pos: 0,
        scroll: 0,
        cache: HashMap::new(),
    };

    loop {
        let description = app.description();
        terminal.draw(|f| draw(f, &app, description))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(None);
        }

        match key.code {
            KeyCode::Up => app.move_by(-1),
            KeyCode::Down => app.move_by(1),
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(5),
            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(5),
            KeyCode::Enter => {
                if app.filtering {
                    app.filtering = false;
                } else if let Some(i) = app.current() {
                    return Ok(Some(entries[i].file.clone()));
                }
            }
            KeyCode::Esc => {
                if app.filtering || !app.filter.is_empty() {
                    app.filtering = false;
                    app.filter.clear();
                    app.reset_selection();
                } else {
                    return Ok(None);
                }
            }
            KeyCode::Backspace if app.filtering => {
                app.filter.pop();
                app.reset_selection();
            }
            KeyCode::Char(c) if app.filtering => {
                app.filter.push(c);
                app.reset_selection();
            }
            KeyCode::Char('/') => app.filtering = true,
            KeyCode::Char('q') => return Ok(None),
            KeyCode::Char('k') => app.move_by(-1),
            KeyCode::Char('j') => app.move_by(1),
            KeyCode::Home => {
                app.pos = 0;
                app.scroll = 0;
            }
            KeyCode::End => {
                app.pos = app.visible().len().saturating_sub(1);
                app.scroll = 0;
            }
            _ => {}
        }
    }
}

/// Seletor interativo de scripts com preview da descrição (markdown).
/// Retorna `None` se o usuário cancelar.
pub fn pick_script(entries: &[ScriptEntry]) -> Result<Option<String>> {
    if entries.is_empty() {
        bail!("Nenhum script para selecionar.");
    }

    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        let names: Vec<&str> = entries.iter().map(|e| e.file.as_str()).collect();
        let selection = Select::new()
            .with_prompt("Qual script deseja executar?")
            .items(&names)
            .default(0)
            .interact()?;
        return Ok(Some(names[selection].to_string()));
    }

    let mut terminal = ratatui::try_init()?;
    let result = run(&mut terminal, entries);
    ratatui::restore();
    result
}
