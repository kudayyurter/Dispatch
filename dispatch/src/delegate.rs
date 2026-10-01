//! `dispatch delegate`: asking Dispatch to run one task in a second agent.
//!
//! Runs inside a pane, as a command the agent there executes. It blocks until
//! the subagent has finished, prints what that subagent printed, and exits with
//! its code — the shape of every other tool an agent runs.
//!
//! Status lines go to stderr and the subagent's output to stdout, so
//! `dispatch delegate --handoff brief.md > result.md` captures the work and nothing else while
//! a human watching the pane still sees progress.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dispatch_client::{Client, ClientError};
use dispatch_core::PaneId;
use dispatch_proto::{ClientMessage, DelegateOutcome, ProtocolError, Role, ServerMessage};

/// Exit codes, following `sysexits(3)` so an agent can branch without parsing
/// prose. Documented in `--help` because an agent reads that far more often
/// than a README.
pub(crate) mod exit {
    /// The handoff is missing, incomplete, or over the limit.
    pub const USAGE: u8 = 64;
    /// The handoff, or a report, cannot be read.
    pub const NOINPUT: u8 = 66;
    /// No daemon is listening.
    pub const UNAVAILABLE: u8 = 69;
    /// The request was never answered, the subagent's pane was closed under it
    /// rather than allowed to exit, or the daemon does not know the pane that
    /// asked. All three mean the work has no verdict and asking again may well
    /// get one.
    pub const TEMPFAIL: u8 = 75;
    /// The user said no.
    pub const NOPERM: u8 = 77;
    /// A cap, or a harness with no task form.
    pub const CONFIG: u8 = 78;
}

/// What `dispatch delegate` was asked to do.
pub struct Args {
    pub harness: Option<String>,
    pub size: (u16, u16),
    pub task: Option<String>,
    pub handoff: Option<String>,
    pub template: bool,
    pub interactive: bool,
}

/// Reads `source`, a path or `-` for standard input, as text.
///
/// Read as bytes and converted lossily: one stray byte in what an agent
/// piped in should not cost it the whole message.
pub(crate) fn read_source(source: &str) -> std::io::Result<String> {
    use std::io::Read;

    let bytes = if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(source)?
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Says why a handoff was not accepted, then prints the template, so an
/// agent can retry in the right shape from the error alone.
fn refuse_handoff(why: &str) -> ExitCode {
    eprintln!("[dispatch] {why}");
    eprintln!("[dispatch] the template (`dispatch delegate --template` prints it again):\n");
    eprint!("{}", dispatch_core::handoff::TEMPLATE);
    ExitCode::from(exit::USAGE)
}

/// Runs one delegation to completion.
pub fn run(args: Args) -> Result<ExitCode> {
    if args.template {
        print!("{}", dispatch_core::handoff::TEMPLATE);
        return Ok(ExitCode::SUCCESS);
    }
    if args.task.is_some() {
        return Ok(refuse_handoff(
            "a bare task is no longer accepted: write a handoff and pass it with \
             --handoff FILE, or --handoff - for standard input",
        ));
    }
    let Some(source) = args.handoff else {
        return Ok(refuse_handoff(
            "no handoff: pass one with --handoff FILE, or --handoff - for standard input",
        ));
    };
    let text = match read_source(&source) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("[dispatch] cannot read the handoff {source}: {error}");
            return Ok(ExitCode::from(exit::NOINPUT));
        }
    };
    let handoff = match dispatch_core::Handoff::parse(&text) {
        Ok(handoff) => handoff,
        Err(error) => {
            let problems: Vec<String> = error.problems.iter().map(|p| format!("  - {p}")).collect();
            return Ok(refuse_handoff(&format!(
                "the handoff is not ready:\n{}",
                problems.join("\n")
            )));
        }
    };
    let interactive = args.interactive;

    let parent: PaneId = std::env::var("DISPATCH_PANE")
        .context("DISPATCH_PANE is not set: `dispatch delegate` runs inside a Dispatch pane")?
        .parse()
        .context("DISPATCH_PANE does not name a pane")?;

    let client = match Client::attach_as(Role::Delegate, "dispatch delegate") {
        Ok(client) => client,
        Err(ClientError::NotRunning(endpoint)) => {
            eprintln!("[dispatch] no daemon is listening on {endpoint}");
            return Ok(ExitCode::from(exit::UNAVAILABLE));
        }
        Err(error) => return Err(error).context("failed to reach the daemon"),
    };

    // The parent's own harness when none was named: the common case is an agent
    // delegating to another of itself. The daemon resolves an empty string to
    // whichever harness the asking pane is running.
    let harness = args.harness.unwrap_or_default();

    client.send(ClientMessage::DelegateRequest {
        parent,
        harness,
        task: handoff.text.clone(),
        size: args.size,
        handoff: Some(handoff),
        interactive,
    });
    eprintln!("[dispatch] waiting for approval (pane {parent})");

    // The daemon owns the approval deadline and refuses a request whose time is
    // up. This one is only a backstop for a daemon that dies without closing its
    // socket cleanly, so it is deliberately longer than any the daemon enforces:
    // the two must never race to answer the same request.
    let backstop = Instant::now() + Duration::from_secs(24 * 60 * 60);

    loop {
        for message in client.poll() {
            match message {
                ServerMessage::DelegateResolved { outcome, .. } => match outcome {
                    DelegateOutcome::Approved { pane } => {
                        eprintln!("[dispatch] approved; subagent pane {pane}");
                    }
                    DelegateOutcome::Denied => {
                        eprintln!("[dispatch] denied");
                        return Ok(ExitCode::from(exit::NOPERM));
                    }
                    DelegateOutcome::Expired { after_secs } => {
                        eprintln!("[dispatch] nobody answered within {after_secs} seconds");
                        return Ok(ExitCode::from(exit::TEMPFAIL));
                    }
                    DelegateOutcome::Refused { reason } => {
                        eprintln!("[dispatch] refused: {reason}");
                        return Ok(ExitCode::from(exit::CONFIG));
                    }
                    DelegateOutcome::Unknown => {
                        eprintln!("[dispatch] unexpected outcome from daemon");
                        return Ok(ExitCode::from(exit::CONFIG));
                    }
                },

                ServerMessage::DelegateFinished {
                    exit: code,
                    tail,
                    report,
                    ..
                } => {
                    use std::io::Write;

                    if let Some(report) = report {
                        std::io::stdout()
                            .write_all(report.as_bytes())
                            .context("failed to write the subagent's report")?;
                        std::io::stdout().flush().ok();
                        eprintln!("[dispatch] the subagent reported");
                        return Ok(ExitCode::SUCCESS);
                    }

                    // -1 is the daemon's sentinel for a subagent that was
                    // killed rather than allowed to exit — its pane was closed
                    // out from under it. No real process exit can produce -1,
                    // and reporting it as ordinary failure (1) would hide that
                    // the work has no verdict at all. TEMPFAIL is the same code
                    // used for an unanswered request, since both mean the same
                    // thing to a caller: try again, this did not finish.
                    if code == -1 {
                        eprintln!("[dispatch] the subagent was stopped before it finished");
                        return Ok(ExitCode::from(exit::TEMPFAIL));
                    }

                    if interactive {
                        eprintln!(
                            "[dispatch] the subagent finished without a report (exit {code}); \
                             its pane has what it did"
                        );
                        return Ok(ExitCode::from(exit::TEMPFAIL));
                    }

                    std::io::stdout()
                        .write_all(&tail)
                        .context("failed to write the subagent's output")?;
                    std::io::stdout().flush().ok();
                    eprintln!("[dispatch] no report was sent; returning the output tail");
                    eprintln!("[dispatch] subagent exited {code}");
                    return Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)));
                }

                // A pane the daemon does not own is the one error here that says
                // "try again" rather than "something is wrong with how this is
                // set up": `DISPATCH_PANE` goes stale the moment the daemon is
                // restarted, and the pane the agent is typing in is a new one
                // with a new id. Telling an agent its configuration is at fault
                // would send it looking for a problem that does not exist —
                // the same argument that gave a timed-out request 75 rather
                // than 78.
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

        // A dropped connection ends the wait, and reconnecting cannot rescue it:
        // the daemon abandons a caller's pending request the moment its socket
        // closes, and a client that reconnects arrives with a new id the answer
        // could not be routed to. Resuming a delegation across a reconnect would
        // mean the daemon holding the request and re-addressing its answer —
        // a feature, not a retry.
        if !client.is_connected() {
            eprintln!("[dispatch] the connection dropped; the request was abandoned with it");
            return Ok(ExitCode::from(exit::TEMPFAIL));
        }

        if Instant::now() >= backstop {
            eprintln!("[dispatch] gave up waiting for the daemon");
            return Ok(ExitCode::from(exit::TEMPFAIL));
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}
