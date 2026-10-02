//! A pane as the daemon sees it.

use dispatch_core::{PaneId, PaneStatus};
use dispatch_pty::Pty;

/// How much of each pane's output the daemon keeps for a client attaching later.
///
/// A client that reattaches is told what a pane printed rather than being given
/// a blank screen, which is the difference between reattaching and starting
/// again. The daemon holds bytes rather than a screen because it has no
/// emulator: it replays what it forwarded.
pub const HISTORY_BYTES: usize = 256 * 1024;

/// One agent owned by the daemon.
///
/// The daemon holds no *drawing* emulator: clients run their own, because they
/// are the ones drawing. It runs one only to answer: whoever owns a program's
/// pseudoterminal must answer the program's questions, and exactly once, which
/// no window can promise when several are attached or none is.
pub struct DaemonPane {
    /// Stable identifier.
    pub id: PaneId,
    /// The running process.
    pub session: Pty,
    /// Which harness is running, so a client attaching later can be told.
    pub harness: String,
    /// The project it belongs to.
    pub project: dispatch_core::ProjectId,
    /// What the pane has printed, up to [`HISTORY_BYTES`].
    pub history: Vec<u8>,
    /// What the pane is doing, as last reported to clients.
    pub status: PaneStatus,
    /// The pane that delegated this one's work.
    pub parent: Option<PaneId>,
    /// Whether it outlives the caller that asked for it.
    pub durable: bool,
    /// The request it answers, while one is waiting.
    pub request: Option<dispatch_core::RequestId>,
    /// Which client is waiting, so its disappearance can end a one-off pane.
    pub caller: Option<u64>,
    /// When the process was seen to exit, if it has.
    ///
    /// The exit and the last of the output are separate events, so a caller
    /// waiting on this pane's output cannot be answered at the exit. This is how
    /// long that wait has lasted.
    pub exited_at: Option<std::time::Instant>,
    /// The file its task was delivered in, while the process may still read
    /// it.
    pub task_file: Option<crate::task_file::TaskFile>,
    /// The branch last reported to clients, so a look that finds the same
    /// one says nothing.
    pub branch: Option<String>,
    /// Whether it runs its harness's own interface rather than its one-shot
    /// form. Such a pane never exits by itself, and is never ended because
    /// its caller went away: the user may be watching it.
    pub interactive: bool,
    /// Whether it has sent its report. A second one is refused.
    pub reported: bool,
    /// Whether it has reported under `interactive_on_done = "ask"` and the
    /// user has not yet answered. Kept apart from `reported`, which never
    /// clears: keeping the pane answers the question, but a second report is
    /// still refused.
    pub awaiting_decision: bool,
    /// Answers the questions the pane's program asks its terminal. `None`
    /// only if one could not be created, in which case the program goes
    /// unanswered, as it did before Dispatch answered at all.
    pub answerer: Option<dispatch_pty::VtTerminal>,
}

impl DaemonPane {
    /// Feeds `output` to the answerer and writes its replies back to the
    /// program.
    pub fn answer(&mut self, output: &[u8]) {
        let Some(answerer) = self.answerer.as_mut() else {
            return;
        };
        answerer.feed(output);
        self.write_replies();
    }

    /// Writes whatever the answerer has to say to the program. Shared by
    /// feeding it output and resizing it, since a resize can produce a reply
    /// of its own (an in-band size report).
    pub fn write_replies(&mut self) {
        let Some(answerer) = self.answerer.as_mut() else {
            return;
        };
        let replies = answerer.take_replies();
        if replies.is_empty() {
            return;
        }
        if let Err(error) = self.session.write(&replies) {
            // Not reading its input: the reply has nowhere to go, and
            // waiting on it would stall every other pane.
            tracing::debug!(pane = %self.id, %error, "dropped a terminal reply");
        }
    }

    /// Records output for a client attaching later.
    ///
    /// The oldest bytes go first once the limit is reached. That can cut an
    /// escape sequence in half, which a terminal parser resynchronises from
    /// within a few bytes; the alternative — keeping everything an agent ever
    /// printed — is unbounded memory.
    pub fn remember(&mut self, output: &[u8]) {
        if output.len() >= HISTORY_BYTES {
            self.history.clear();
            self.history
                .extend_from_slice(&output[output.len() - HISTORY_BYTES..]);
            return;
        }

        self.history.extend_from_slice(output);

        let excess = self.history.len().saturating_sub(HISTORY_BYTES);
        if excess > 0 {
            self.history.drain(..excess);
        }
    }
}
