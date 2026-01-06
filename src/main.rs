mod app;
mod ui;
mod wallpaper;

use app::{App, Mode};
use color_eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::{Block, Borders, Gauge}};
use std::io::{self, stdout};

fn main() -> Result<()> {
    color_eyre::install()?;

    // Setup terminal
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Run app
    let result = run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new()?;

    // Preload all thumbnails with progress
    app.preload_thumbnails(|current, total, name| {
        let _ = terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Percentage(40),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Percentage(40),
            ]).split(area);

            let progress = if total > 0 { current as f64 / total as f64 } else { 0.0 };
            let gauge = Gauge::default()
                .block(Block::default().title(" Loading thumbnails ").borders(Borders::ALL))
                .gauge_style(Style::default().fg(Color::Cyan))
                .ratio(progress)
                .label(format!("{}/{}", current + 1, total));
            frame.render_widget(gauge, chunks[1]);

            let name_text = ratatui::widgets::Paragraph::new(name.to_string())
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(name_text, chunks[2]);
        });
    });

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Handle input modes separately
            match app.mode {
                Mode::Search => match key.code {
                    KeyCode::Esc => app.cancel_search(),
                    KeyCode::Enter => app.confirm_search(),
                    KeyCode::Backspace => app.search_backspace(),
                    KeyCode::Char(c) => app.search_input(c),
                    _ => {}
                },
                Mode::Command => match key.code {
                    KeyCode::Esc => app.cancel_command(),
                    KeyCode::Enter => app.confirm_command()?,
                    KeyCode::Backspace => app.command_backspace(),
                    KeyCode::Tab => app.command_autocomplete(),
                    KeyCode::Up => app.move_completion_up(),
                    KeyCode::Down => app.move_completion_down(),
                    KeyCode::Char(c) => app.command_input(c),
                    _ => {}
                },
                _ => match key.code {
                    // Quit
                    KeyCode::Char('q') => app.should_quit = true,

                    // Navigation - vim bindings
                    KeyCode::Char('h') | KeyCode::Left => app.move_left(),
                    KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                    KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                    KeyCode::Char('l') | KeyCode::Right => app.move_right(),

                    // Search and Command
                    KeyCode::Char('/') => app.start_search(),
                    KeyCode::Char(':') => app.start_command(),

                    // Reset destination
                    KeyCode::Char('H') => app.reset_view_dir()?,

                    // Actions
                    KeyCode::Enter => {
                        app.apply_wallpaper()?;
                    }
                    KeyCode::Char(' ') => app.toggle_preview(),
                    KeyCode::Char('?') => app.toggle_help(),
                    KeyCode::Esc => app.escape(),

                    _ => {}
                },
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
