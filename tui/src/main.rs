mod app;
mod art;
mod devtools;
#[cfg(test)]
mod devtools_tests;
mod devtools_ui;
#[cfg(test)]
mod app_tests;
mod history;
mod keys;
mod spaces;
mod mock;
mod protocol;
mod resolve;
mod session;
mod transport;
mod ui;
mod views;

use app::App;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event};
use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};
use ratatui::Terminal;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Appends a line to `~/Library/Logs/tilde/tui.log`. Never writes to the terminal.
#[cfg(not(test))]
pub fn log(message: &str) {
    use std::fs::{create_dir_all, OpenOptions};
    use std::io::Write;
    let Some(home) = std::env::var_os("HOME") else { return };
    let dir = std::path::PathBuf::from(home).join("Library/Logs/tilde");
    let _ = create_dir_all(&dir);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(dir.join("tui.log")) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file, "{secs} [{}] {message}", std::process::id());
    }
}

#[cfg(test)]
pub fn log(_message: &str) {}

/// How long one frame of an animation is shown.
const FRAME: Duration = Duration::from_millis(80);

enum Input {
    Terminal(Event),
    Line(String),
    Disconnected,
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen, cursor::Show);
}

const USAGE: &str = "tilde-tui: the terminal interface of the Tilde browser

It is started by Tilde.app, which passes TILDE_SOCKET.

Options:
  --mock      run against a built-in fake engine in any terminal
  --version   print the version
  --help      print this help";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mock = false;
    for arg in &args {
        match arg.as_str() {
            "--mock" => mock = true,
            "--version" => {
                println!("tilde-tui {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("tilde-tui: unknown option {other}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let (tx, rx) = mpsc::channel::<Input>();
    let transport: Box<dyn transport::Transport>;
    let session_path;

    if mock {
        transport = Box::new(mock::MockEngine::new());
        // The mock never touches the real session unless asked to.
        session_path = std::env::var_os("TILDE_SESSION").map(std::path::PathBuf::from);
    } else {
        let Some(socket) = std::env::var_os("TILDE_SOCKET").filter(|s| !s.is_empty()) else {
            eprintln!("tilde-tui is the interface of the Tilde browser and is started by Tilde.app. Use --mock to try it in a plain terminal.");
            std::process::exit(2);
        };
        let stream = match UnixStream::connect(&socket) {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("tilde-tui: cannot connect to the app at {}: {error}", socket.to_string_lossy());
                std::process::exit(1);
            }
        };
        let reader = stream.try_clone().expect("clone socket");
        let line_tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                match line {
                    Ok(line) if !line.is_empty() => {
                        if line_tx.send(Input::Line(line)).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = line_tx.send(Input::Disconnected);
        });
        transport = Box::new(transport::SocketTransport::new(stream));
        session_path = session::default_path();
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        log(&format!("panic: {info}"));
        default_hook(info);
    }));

    if let Err(error) = run(transport, session_path, !mock, tx, rx) {
        restore_terminal();
        log(&format!("fatal: {error}"));
        eprintln!("tilde-tui: {error}");
        std::process::exit(1);
    }
}

fn run(
    transport: Box<dyn transport::Transport>,
    session_path: Option<std::path::PathBuf>,
    // The terminal is Tilde's own, so its colours are ours to set.
    own_terminal: bool,
    tx: mpsc::Sender<Input>,
    rx: mpsc::Receiver<Input>,
) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if tx.send(Input::Terminal(event)).is_err() {
                break;
            }
        }
    });

    let mut app = App::new(transport, session_path);
    log("started");
    app.start();

    let mut last_save = Instant::now();
    let started = Instant::now();
    // The colour the terminal shows where there are no cells: the strip
    // left over at the window's edges. It follows the space's background.
    let mut edge = None;
    loop {
        let background = ui::Palette::of(&app).bg;
        if own_terminal && edge != Some(background) {
            edge = Some(background);
            if let ratatui::style::Color::Rgb(r, g, b) = background {
                let _ = write!(io::stdout(), "\x1b]11;rgb:{r:02x}/{g:02x}/{b:02x}\x07");
            }
        }
        terminal.draw(|frame| {
            app.set_area(frame.area());
            if let Some(position) = ui::draw(frame, &app) {
                frame.set_cursor_position(position);
            }
        })?;

        if app.should_quit {
            break;
        }

        // Wakes when a tab waiting to be opened is due, and at least every
        // 250 ms, or for every frame while something is animated.
        let idle = if app.animating() { FRAME } else { Duration::from_millis(250) };
        let wait = app.deadline().map_or(idle, |at| at.saturating_duration_since(Instant::now()).min(idle));
        let mut next = rx.recv_timeout(wait).ok();
        while let Some(input) = next {
            match input {
                Input::Terminal(Event::Key(key)) => app.handle_key(key),
                Input::Terminal(Event::Mouse(mouse)) => app.handle_mouse(mouse),
                Input::Terminal(Event::Paste(text)) => app.handle_paste(&text),
                Input::Terminal(_) => {}
                Input::Line(line) => app.handle_line(&line),
                Input::Disconnected => {
                    app.save_session();
                    restore_terminal();
                    eprintln!("tilde-tui: lost the connection to the app.");
                    std::process::exit(1);
                }
            }
            if app.should_quit {
                break;
            }
            next = rx.try_recv().ok();
        }

        app.tick(Instant::now());
        app.frame = (started.elapsed().as_millis() / FRAME.as_millis()) as usize;

        if (app.session_dirty || app.history.dirty) && last_save.elapsed() > Duration::from_millis(500) {
            app.save_session();
            last_save = Instant::now();
        }
    }

    restore_terminal();
    log("quit");
    Ok(())
}
