//! Daemon-backed terminal session
//!
//! Provides `DaemonTab` which wraps a daemon session with a local Terminal
//! for rendering. The Terminal has no PTY — input is forwarded to the daemon
//! and raw output is streamed back and fed through the local parser/screen.

use cterm_client::{ClientError, CreateSessionOpts, DaemonConnection, SessionHandle};
use cterm_core::pty::PtyError;
use cterm_core::screen::ScreenConfig;
use cterm_core::term::{Terminal, WriteFn};
use std::sync::Arc;

use crate::session::next_tab_id;

/// A tab backed by a daemon session
pub struct DaemonTab {
    /// Unique tab ID (same ID space as local tabs)
    pub id: u64,
    /// Local terminal for screen state and rendering (no PTY)
    pub terminal: Terminal,
    /// Handle to the daemon session
    pub session: SessionHandle,
    /// Tab title
    pub title: String,
    /// Custom title set by user
    pub custom_title: Option<String>,
    /// Tab color override
    pub color: Option<String>,
    /// Whether there's unread output
    pub has_unread: bool,
    /// Whether the remote process is still running
    pub running: bool,
}

impl DaemonTab {
    /// Create a new daemon-backed tab by creating a session on the daemon
    pub async fn new(
        conn: &DaemonConnection,
        cols: usize,
        rows: usize,
    ) -> Result<Self, DaemonTabError> {
        let session = conn
            .create_session(CreateSessionOpts {
                cols: cols as u32,
                rows: rows as u32,
                ..Default::default()
            })
            .await?;

        let mut terminal = Terminal::new(cols, rows, ScreenConfig::default());

        // Set up write callback to forward input to daemon
        let write_session = session.clone();
        let write_fn: WriteFn = Box::new(move |data: &[u8]| {
            let session = write_session.clone();
            let data = data.to_vec();
            // Fire-and-forget async write from sync context
            tokio::spawn(async move {
                if let Err(e) = session.write_input(&data).await {
                    log::error!("Failed to write to daemon session: {}", e);
                }
            });
            Ok(())
        });
        terminal.set_write_fn(write_fn);

        Ok(Self {
            id: next_tab_id(),
            terminal,
            session,
            title: "Terminal".into(),
            custom_title: None,
            color: None,
            has_unread: false,
            running: true,
        })
    }

    /// Create a new daemon-backed tab with a specific command
    pub async fn with_command(
        conn: &DaemonConnection,
        command: &str,
        args: &[String],
        cwd: Option<String>,
        cols: usize,
        rows: usize,
    ) -> Result<Self, DaemonTabError> {
        let session = conn
            .create_session(CreateSessionOpts {
                cols: cols as u32,
                rows: rows as u32,
                shell: Some(command.to_string()),
                args: args.to_vec(),
                cwd,
                ..Default::default()
            })
            .await?;

        let mut terminal = Terminal::new(cols, rows, ScreenConfig::default());

        // Set up write callback
        let write_session = session.clone();
        let write_fn: WriteFn = Box::new(move |data: &[u8]| {
            let session = write_session.clone();
            let data = data.to_vec();
            tokio::spawn(async move {
                if let Err(e) = session.write_input(&data).await {
                    log::error!("Failed to write to daemon session: {}", e);
                }
            });
            Ok(())
        });
        terminal.set_write_fn(write_fn);

        Ok(Self {
            id: next_tab_id(),
            terminal,
            session,
            title: command.to_string(),
            custom_title: None,
            color: None,
            has_unread: false,
            running: true,
        })
    }

    /// Attach to an existing daemon session
    pub async fn attach(
        conn: &DaemonConnection,
        session_id: &str,
        cols: usize,
        rows: usize,
    ) -> Result<Self, DaemonTabError> {
        let (session, initial_screen) = conn
            .attach_session(session_id, cols as u32, rows as u32)
            .await?;

        let mut terminal = Terminal::new(cols, rows, ScreenConfig::default());

        // If we got an initial screen snapshot, replay it
        if let Some(screen_data) = initial_screen {
            // Feed the screen data into the terminal
            // The screen data from attach contains the full screen state as proto
            // We'll handle this by applying the proto screen data directly
            apply_screen_snapshot(&mut terminal, &screen_data);
        }

        // Set up write callback
        let write_session = session.clone();
        let write_fn: WriteFn = Box::new(move |data: &[u8]| {
            let session = write_session.clone();
            let data = data.to_vec();
            tokio::spawn(async move {
                if let Err(e) = session.write_input(&data).await {
                    log::error!("Failed to write to daemon session: {}", e);
                }
            });
            Ok(())
        });
        terminal.set_write_fn(write_fn);

        Ok(Self {
            id: next_tab_id(),
            terminal,
            session,
            title: "Terminal".into(),
            custom_title: None,
            color: None,
            has_unread: false,
            running: true,
        })
    }

    /// Get the display title
    pub fn display_title(&self) -> &str {
        self.custom_title.as_ref().unwrap_or(&self.title)
    }

    /// Whether this is a remote session
    pub fn is_remote(&self) -> bool {
        self.session.is_remote()
    }

    /// Get the hostname of the daemon
    pub fn hostname(&self) -> &str {
        self.session.hostname()
    }

    /// Get the daemon session ID
    pub fn session_id(&self) -> &str {
        self.session.session_id()
    }

    /// Detach from this session (keep running in background)
    pub async fn detach(&self) -> Result<(), DaemonTabError> {
        self.session.detach().await?;
        Ok(())
    }
}

/// Apply a proto screen snapshot to a local terminal.
///
/// Sets cursor position and title. Full screen content is synced
/// via stream_output which feeds raw PTY data through the parser.
fn apply_screen_snapshot(
    terminal: &mut Terminal,
    screen_data: &cterm_proto::proto::GetScreenResponse,
) {
    if let Some(cursor) = &screen_data.cursor {
        terminal.screen_mut().cursor.row = cursor.row as usize;
        terminal.screen_mut().cursor.col = cursor.col as usize;
    }

    if !screen_data.title.is_empty() {
        terminal.screen_mut().title = screen_data.title.clone();
    }
}

/// Start the output streaming loop for a daemon tab.
///
/// This spawns a background task that streams raw PTY output from the daemon
/// and feeds it into the local Terminal's parser, keeping the screen in sync.
///
/// Returns a join handle for the background task.
pub fn spawn_output_stream(
    session: SessionHandle,
    terminal: Arc<parking_lot::Mutex<Terminal>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        match session.stream_output().await {
            Ok(mut stream) => {
                use tokio_stream::StreamExt;
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(chunk) => {
                            let mut term = terminal.lock();
                            let events = term.process(&chunk.data);
                            // Handle events
                            for event in events {
                                match event {
                                    cterm_core::term::TerminalEvent::TitleChanged(title) => {
                                        log::debug!("Daemon session title: {}", title);
                                    }
                                    cterm_core::term::TerminalEvent::Bell => {
                                        log::debug!("Daemon session bell");
                                    }
                                    cterm_core::term::TerminalEvent::ProcessExited(code) => {
                                        log::info!("Daemon session process exited: {}", code);
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Error streaming daemon output: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to start daemon output stream: {}", e);
            }
        }
    })
}

/// Error type for daemon tab operations
#[derive(Debug, thiserror::Error)]
pub enum DaemonTabError {
    #[error("Client error: {0}")]
    Client(#[from] ClientError),

    #[error("PTY error: {0}")]
    Pty(#[from] PtyError),
}
