//! `dispatch report`: a subagent hands its work back.
//!
//! Runs inside the subagent's pane, as the last thing it does. The daemon
//! passes the report to the `dispatch delegate` waiting on this pane, which
//! prints it for the agent that asked.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dispatch_client::{Client, ClientError};
use dispatch_core::PaneId;
use dispatch_proto::{ClientMessage, ProtocolError, ReportOutcome, Role, ServerMessage};

use crate::delegate::{exit, read_source};

/// How long to wait for the daemon's answer. It answers at once; this only
/// covers a daemon that died without closing its socket.
const PATIENCE: Duration = Duration::from_secs(60);

/// Sends one report.
pub fn run(file: &str) -> Result<ExitCode> {
    let report = match read_source(file) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("[dispatch] cannot read the report {file}: {error}");
            return Ok(ExitCode::from(exit::NOINPUT));
        }
    };
    if report.trim().is_empty() {
        eprintln!("[dispatch] the report is empty; write what the handoff's Report back asked for");
        return Ok(ExitCode::from(exit::USAGE));
    }

    let pane: PaneId = std::env::var("DISPATCH_PANE")
        .context("DISPATCH_PANE is not set: `dispatch report` runs inside a subagent's pane")?
        .parse()
        .context("DISPATCH_PANE does not name a pane")?;

    let client = match Client::attach_as(Role::Delegate, "dispatch report") {
        Ok(client) => client,
        Err(ClientError::NotRunning(endpoint)) => {
            eprintln!("[dispatch] no daemon is listening on {endpoint}");
            return Ok(ExitCode::from(exit::UNAVAILABLE));
        }
        Err(error) => return Err(error).context("failed to reach the daemon"),
    };

    client.send(ClientMessage::DelegateReport { pane, report });

    let deadline = Instant::now() + PATIENCE;
    loop {
        for message in client.poll() {
            match message {
                ServerMessage::ReportAnswered { outcome } => {
                    return Ok(match outcome {
                        ReportOutcome::Delivered => {
                            eprintln!("[dispatch] report delivered");
                            ExitCode::SUCCESS
                        }
                        ReportOutcome::NotWaiting { reason } => {
                            eprintln!("[dispatch] {reason}");
                            ExitCode::from(exit::TEMPFAIL)
                        }
                        ReportOutcome::Refused { reason } => {
                            eprintln!("[dispatch] refused: {reason}");
                            ExitCode::from(exit::CONFIG)
                        }
                        ReportOutcome::Unknown => {
                            eprintln!("[dispatch] unexpected answer from the daemon");
                            ExitCode::from(exit::CONFIG)
                        }
                    });
                }
                ServerMessage::Error {
                    error: ProtocolError::NoSuchPane(pane),
                } => {
                    eprintln!("[dispatch] the daemon does not know pane {pane}");
                    return Ok(ExitCode::from(exit::TEMPFAIL));
                }
                ServerMessage::Error { error } => {
                    eprintln!("[dispatch] {error}");
                    return Ok(ExitCode::from(exit::CONFIG));
                }
                _ => {}
            }
        }

        if !client.is_connected() || Instant::now() >= deadline {
            eprintln!("[dispatch] the daemon did not answer; the report may not have arrived");
            return Ok(ExitCode::from(exit::TEMPFAIL));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
