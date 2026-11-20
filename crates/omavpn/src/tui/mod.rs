pub mod app;
pub mod ui;

use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use omavpn_core::storage::Storage;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::client::DaemonClient;
use crate::tui::app::App;

pub async fn run(storage: Storage, client: DaemonClient) -> Result<()> {
    let mut app = App::new(storage, client).await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    res
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let tick_rate = Duration::from_secs(2);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if app.should_quit {
            break;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout.min(Duration::from_millis(100)))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key).await,
                Event::Paste(text) => app.handle_paste(text),
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.refresh_daemon().await;
            last_tick = Instant::now();
        }
    }

    Ok(())
}
