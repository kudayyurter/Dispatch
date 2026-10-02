//! Deciding whether a delegation request may be asked about at all.
//!
//! These rules refuse without prompting. A prompt for the five-hundredth
//! request is not a safeguard; it is a way to make someone hold down `d`. The
//! user's judgement is for requests that are plausible.

use std::time::Instant;

use dispatch_config::DelegationLimits;
use dispatch_core::{PaneId, RequestId};
use dispatch_proto::ServerMessage;

/// A request that has been asked about and not yet answered.
pub struct Pending {
    /// The request.
    pub id: RequestId,
    /// The pane that asked.
    pub parent: PaneId,
    /// Which harness would run.
    pub harness: String,
    /// What it would be asked to do.
    pub handoff: dispatch_core::Handoff,
    /// Whether it would run in its harness's own interface.
    pub interactive: bool,
    /// Size to start the subagent at.
    pub size: (u16, u16),
    /// Which client is waiting for the answer.
    pub caller: u64,
    /// When it was asked, for the deadline.
    pub asked: Instant,
    /// What the interface clients were told, so a late subscriber can be sent
    /// the same thing without rebuilding it.
    pub announcement: ServerMessage,
}

/// Why a request with no handoff is refused.
pub const NO_HANDOFF: &str = "a bare task is not enough to delegate with: write a handoff \
    (`dispatch delegate --template` prints one) and pass it with `--handoff FILE`, or \
    `--handoff -` for standard input";

/// Why a request cannot be asked about, if it cannot.
///
/// `depth` is how many parents the asking pane already has, `live` how many of
/// its children are still working for it (their requests are open), and `has_task_form` whether the harness has a
/// non-interactive shape to run at all (or, for an interactive request, an
/// interactive one).
#[must_use]
pub fn refusal(
    depth: u8,
    live: usize,
    limits: DelegationLimits,
    has_task_form: bool,
    harness: &str,
    interactive: bool,
) -> Option<String> {
    if !has_task_form {
        return Some(if interactive {
            format!(
                "harness {harness:?} has no [task.interactive] form, so it cannot run as an \
                 interactive subagent; add one or delegate without --interactive"
            )
        } else {
            format!(
                "harness {harness:?} has no [task] form, so it cannot be run on one task; \
                 add one or delegate to a harness that has one"
            )
        });
    }

    if depth >= limits.max_depth {
        return Some(format!(
            "delegation is capped at depth {}; this pane is already a subagent",
            limits.max_depth
        ));
    }

    if live >= limits.max_live_per_parent {
        return Some(format!(
            "this pane already has {live} subagents working for it, and the cap is {}",
            limits.max_live_per_parent
        ));
    }

    None
}

#[cfg(test)]
mod tests;
