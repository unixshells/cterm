//! gRPC TerminalService implementation

use crate::convert::{
    cell_to_proto, event_to_proto, proto_to_key, proto_to_modifiers, screen_to_proto,
    screen_to_text,
};
use crate::proto::terminal_service_server::TerminalService;
use crate::proto::*;
use crate::session::SessionManager;
#[cfg(unix)]
use libc;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::{
    wrappers::errors::BroadcastStreamRecvError, wrappers::BroadcastStream, Stream, StreamExt,
};
use tonic::{Request, Response, Status};

/// TerminalService implementation
pub struct TerminalServiceImpl {
    session_manager: Arc<SessionManager>,
}

impl TerminalServiceImpl {
    /// Create a new TerminalService
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self { session_manager }
    }
}

#[tonic::async_trait]
impl TerminalService for TerminalServiceImpl {
    // ========================================================================
    // Session Management
    // ========================================================================

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        let req = request.into_inner();

        let cols = req.cols.max(1) as usize;
        let rows = req.rows.max(1) as usize;

        let env: Vec<(String, String)> = req.env.into_iter().collect();

        let session = self
            .session_manager
            .create_session(
                cols,
                rows,
                req.shell,
                req.args,
                req.cwd.map(PathBuf::from),
                env,
                req.term,
            )
            .map_err(Status::from)?;

        Ok(Response::new(CreateSessionResponse {
            session_id: session.id.clone(),
            cols: cols as u32,
            rows: rows as u32,
        }))
    }

    async fn list_sessions(
        &self,
        _request: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let sessions = self.session_manager.list_sessions();

        let session_infos: Vec<SessionInfo> = sessions
            .iter()
            .map(|s| {
                let (cols, rows) = s.dimensions();
                SessionInfo {
                    session_id: s.id.clone(),
                    cols: cols as u32,
                    rows: rows as u32,
                    title: s.title(),
                    running: s.is_running(),
                    child_pid: s.child_pid().unwrap_or(0),
                    attached_clients: 0, // TODO: track attached clients
                }
            })
            .collect();

        Ok(Response::new(ListSessionsResponse {
            sessions: session_infos,
        }))
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let (cols, rows) = session.dimensions();
        let info = SessionInfo {
            session_id: session.id.clone(),
            cols: cols as u32,
            rows: rows as u32,
            title: session.title(),
            running: session.is_running(),
            child_pid: session.child_pid().unwrap_or(0),
            attached_clients: 0, // TODO: track attached clients
        };

        Ok(Response::new(GetSessionResponse {
            session: Some(info),
        }))
    }

    async fn destroy_session(
        &self,
        request: Request<DestroySessionRequest>,
    ) -> Result<Response<DestroySessionResponse>, Status> {
        let req = request.into_inner();
        self.session_manager
            .destroy_session(&req.session_id, req.signal)
            .map_err(Status::from)?;

        Ok(Response::new(DestroySessionResponse { success: true }))
    }

    // ========================================================================
    // Input
    // ========================================================================

    async fn write_input(
        &self,
        request: Request<WriteInputRequest>,
    ) -> Result<Response<WriteInputResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let bytes_written = session.write_input(&req.data).map_err(Status::from)?;

        Ok(Response::new(WriteInputResponse {
            bytes_written: bytes_written as u32,
        }))
    }

    async fn send_key(
        &self,
        request: Request<SendKeyRequest>,
    ) -> Result<Response<SendKeyResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let key = req
            .key
            .as_ref()
            .and_then(proto_to_key)
            .ok_or_else(|| Status::invalid_argument("Invalid key"))?;

        let modifiers = req
            .modifiers
            .as_ref()
            .map(proto_to_modifiers)
            .unwrap_or_default();

        let sequence = session.handle_key(key, modifiers).unwrap_or_default();

        // Write the sequence to the PTY
        if !sequence.is_empty() {
            session.write_input(&sequence).map_err(Status::from)?;
        }

        Ok(Response::new(SendKeyResponse { sequence }))
    }

    // ========================================================================
    // Output Streaming
    // ========================================================================

    type StreamOutputStream =
        Pin<Box<dyn Stream<Item = Result<OutputChunk, Status>> + Send + 'static>>;

    async fn stream_output(
        &self,
        request: Request<StreamOutputRequest>,
    ) -> Result<Response<Self::StreamOutputStream>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let rx = session.subscribe_output();
        let stream = BroadcastStream::new(rx).filter_map(|result| {
            match result {
                Ok(data) => Some(Ok(OutputChunk {
                    data: data.data,
                    timestamp_ms: data.timestamp_ms,
                })),
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    // Skip lagged messages
                    None
                }
            }
        });

        Ok(Response::new(Box::pin(stream)))
    }

    // ========================================================================
    // Screen State
    // ========================================================================

    async fn get_screen(
        &self,
        request: Request<GetScreenRequest>,
    ) -> Result<Response<GetScreenResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let response =
            session.with_terminal(|term| screen_to_proto(term.screen(), req.include_scrollback));

        Ok(Response::new(response))
    }

    async fn get_cell(
        &self,
        request: Request<GetCellRequest>,
    ) -> Result<Response<GetCellResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let cell = session.with_terminal(|term| {
            term.screen()
                .get_cell(req.row as usize, req.col as usize)
                .cloned()
        });

        let cell = cell.ok_or_else(|| Status::out_of_range("Cell position out of range"))?;

        Ok(Response::new(GetCellResponse {
            cell: Some(cell_to_proto(&cell)),
        }))
    }

    async fn get_cursor(
        &self,
        request: Request<GetCursorRequest>,
    ) -> Result<Response<GetCursorResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let cursor = session.with_terminal(|term| {
            let screen = term.screen();
            CursorPosition {
                row: screen.cursor.row as u32,
                col: screen.cursor.col as u32,
                visible: screen.modes.show_cursor,
                style: CursorStyle::Block as i32,
            }
        });

        Ok(Response::new(GetCursorResponse {
            cursor: Some(cursor),
        }))
    }

    async fn get_screen_text(
        &self,
        request: Request<GetScreenTextRequest>,
    ) -> Result<Response<GetScreenTextResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let lines = session.with_terminal(|term| {
            screen_to_text(
                term.screen(),
                req.include_scrollback,
                req.start_row,
                req.end_row,
            )
        });

        Ok(Response::new(GetScreenTextResponse { lines }))
    }

    // ========================================================================
    // Control
    // ========================================================================

    async fn resize(
        &self,
        request: Request<ResizeRequest>,
    ) -> Result<Response<ResizeResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        session.resize(req.cols as usize, req.rows as usize);

        Ok(Response::new(ResizeResponse { success: true }))
    }

    async fn send_signal(
        &self,
        request: Request<SendSignalRequest>,
    ) -> Result<Response<SendSignalResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        session.send_signal(req.signal).map_err(Status::from)?;

        Ok(Response::new(SendSignalResponse { success: true }))
    }

    // ========================================================================
    // Event Streaming
    // ========================================================================

    type StreamEventsStream =
        Pin<Box<dyn Stream<Item = Result<TerminalEvent, Status>> + Send + 'static>>;

    async fn stream_events(
        &self,
        request: Request<StreamEventsRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        let rx = session.subscribe_events();
        let stream = BroadcastStream::new(rx).filter_map(|result| match result {
            Ok(event) => Some(Ok(event_to_proto(&event))),
            Err(BroadcastStreamRecvError::Lagged(_)) => None,
        });

        Ok(Response::new(Box::pin(stream)))
    }

    // ========================================================================
    // Connection Management (new RPCs)
    // ========================================================================

    async fn handshake(
        &self,
        request: Request<HandshakeRequest>,
    ) -> Result<Response<HandshakeResponse>, Status> {
        let req = request.into_inner();
        log::info!(
            "Client connected: {} (version {})",
            req.client_id,
            req.client_version
        );

        let hostname = gethostname();

        Ok(Response::new(HandshakeResponse {
            daemon_id: String::new(), // TODO: generate daemon ID
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            is_local: true, // TODO: detect from transport
            hostname,
            protocol_version: 1,
        }))
    }

    async fn attach_session(
        &self,
        request: Request<AttachSessionRequest>,
    ) -> Result<Response<AttachSessionResponse>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        // Resize to client dimensions if provided
        if req.cols > 0 && req.rows > 0 {
            session.resize(req.cols as usize, req.rows as usize);
        }

        let (cols, rows) = session.dimensions();
        let info = SessionInfo {
            session_id: session.id.clone(),
            cols: cols as u32,
            rows: rows as u32,
            title: session.title(),
            running: session.is_running(),
            child_pid: session.child_pid().unwrap_or(0),
            attached_clients: 1, // TODO: track properly
        };

        let initial_screen = if req.want_screen_snapshot {
            Some(session.with_terminal(|term| screen_to_proto(term.screen(), true)))
        } else {
            None
        };

        Ok(Response::new(AttachSessionResponse {
            session: Some(info),
            initial_screen,
        }))
    }

    async fn detach_session(
        &self,
        request: Request<DetachSessionRequest>,
    ) -> Result<Response<DetachSessionResponse>, Status> {
        let req = request.into_inner();

        if !req.keep_running {
            // Destroy the session
            self.session_manager
                .destroy_session(&req.session_id, None)
                .map_err(Status::from)?;
        }
        // TODO: track detach state

        Ok(Response::new(DetachSessionResponse { success: true }))
    }

    // ========================================================================
    // Screen Update Streaming
    // ========================================================================

    type StreamScreenUpdatesStream =
        Pin<Box<dyn Stream<Item = Result<ScreenUpdate, Status>> + Send + 'static>>;

    async fn stream_screen_updates(
        &self,
        request: Request<StreamScreenUpdatesRequest>,
    ) -> Result<Response<Self::StreamScreenUpdatesStream>, Status> {
        let req = request.into_inner();
        let session = self
            .session_manager
            .get_session(&req.session_id)
            .map_err(Status::from)?;

        // For now, send a full screen update on each ContentChanged event
        let session_id = req.session_id.clone();
        let rx = session.subscribe_events();
        let session_ref = session.clone();
        let mut seq: u64 = 0;

        let stream = BroadcastStream::new(rx).filter_map(move |result| match result {
            Ok(event) => {
                if matches!(event, cterm_core::term::TerminalEvent::ContentChanged) {
                    seq += 1;
                    let screen_data =
                        session_ref.with_terminal(|term| screen_to_proto(term.screen(), false));
                    Some(Ok(ScreenUpdate {
                        session_id: session_id.clone(),
                        sequence: seq,
                        update_type: Some(screen_update::UpdateType::FullScreen(
                            FullScreenUpdate {
                                screen: Some(screen_data),
                            },
                        )),
                    }))
                } else if matches!(event, cterm_core::term::TerminalEvent::TitleChanged(_)) {
                    seq += 1;
                    let title = session_ref.title();
                    Some(Ok(ScreenUpdate {
                        session_id: session_id.clone(),
                        sequence: seq,
                        update_type: Some(screen_update::UpdateType::Title(TitleUpdate { title })),
                    }))
                } else {
                    None
                }
            }
            Err(BroadcastStreamRecvError::Lagged(_)) => None,
        });

        Ok(Response::new(Box::pin(stream)))
    }

    // ========================================================================
    // Daemon Management
    // ========================================================================

    async fn get_daemon_info(
        &self,
        _request: Request<GetDaemonInfoRequest>,
    ) -> Result<Response<GetDaemonInfoResponse>, Status> {
        let hostname = gethostname();

        Ok(Response::new(GetDaemonInfoResponse {
            daemon_id: String::new(), // TODO
            version: env!("CARGO_PKG_VERSION").to_string(),
            hostname,
            session_count: self.session_manager.session_count() as u32,
            client_count: 0, // TODO: track connected clients
            uptime_secs: 0,  // TODO: track uptime
        }))
    }

    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        let req = request.into_inner();

        if !req.force && self.session_manager.session_count() > 0 {
            return Ok(Response::new(ShutdownResponse {
                success: false,
                reason: "Active sessions exist. Use force=true to override.".to_string(),
            }));
        }

        // TODO: trigger actual shutdown
        log::info!("Shutdown requested (force={})", req.force);

        Ok(Response::new(ShutdownResponse {
            success: true,
            reason: String::new(),
        }))
    }
}

fn gethostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut _, buf.len()) } == 0 {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..len]).to_string()
        } else {
            "unknown".to_string()
        }
    }
    #[cfg(not(unix))]
    {
        "unknown".to_string()
    }
}
