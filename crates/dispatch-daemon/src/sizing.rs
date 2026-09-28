//! Which window's size each pane takes, when several windows show it.
//!
//! Every window asks for the size of its own tile, and a pty has one size.
//! The window the user is using decides: each client is ranked by when it
//! was last used, and a pane takes the size its highest-ranked asker wants.
//! Kept apart from the daemon loop so the rule can be tested without one.

use std::collections::HashMap;

use dispatch_core::PaneId;
use dispatch_pty::Size;

/// Each window's asked size for each pane, and how recently each was used.
#[derive(Debug, Default)]
pub(crate) struct Sizes {
    asked: HashMap<(u64, PaneId), Size>,
    ranks: HashMap<u64, u64>,
    /// The last rank handed out; the next use gets one more.
    counter: u64,
}

impl Sizes {
    /// Marks `client` as the window in use now.
    ///
    /// Returns the panes whose size may have changed: those `client` asked a
    /// size for, or none when it was the window in use already.
    pub(crate) fn touch(&mut self, client: u64) -> Vec<PaneId> {
        let top = self.ranks.values().max().copied();
        let was = self.ranks.get(&client).copied();

        self.counter += 1;
        self.ranks.insert(client, self.counter);

        if was.is_some() && was == top {
            return Vec::new();
        }
        self.panes_asked_by(client)
    }

    /// Records that `client` shows `pane` in a tile of `size`.
    pub(crate) fn ask(&mut self, client: u64, pane: PaneId, size: Size) {
        self.asked.insert((client, pane), size);
    }

    /// Records that `client` no longer shows `pane`.
    pub(crate) fn hide(&mut self, client: u64, pane: PaneId) {
        self.asked.remove(&(client, pane));
    }

    /// Forgets a window that has gone, returning the panes it asked a size
    /// for, whose size may now change.
    pub(crate) fn forget_client(&mut self, client: u64) -> Vec<PaneId> {
        let panes = self.panes_asked_by(client);
        self.asked.retain(|(asker, _), _| *asker != client);
        self.ranks.remove(&client);
        panes
    }

    /// Forgets a pane that has closed.
    pub(crate) fn forget_pane(&mut self, pane: PaneId) {
        self.asked.retain(|(_, asked), _| *asked != pane);
    }

    /// The size `pane` should have: the one its most recently used asker
    /// wants, or `None` when no window has asked.
    ///
    /// A window never used ranks below every window that was. Between
    /// equals, the later client, the higher id, decides.
    pub(crate) fn wanted(&self, pane: PaneId) -> Option<Size> {
        self.asked
            .iter()
            .filter(|((_, asked), _)| *asked == pane)
            .max_by_key(|((client, _), _)| (self.ranks.get(client).copied().unwrap_or(0), *client))
            .map(|(_, size)| *size)
    }

    fn panes_asked_by(&self, client: u64) -> Vec<PaneId> {
        self.asked
            .keys()
            .filter(|(asker, _)| *asker == client)
            .map(|(_, pane)| *pane)
            .collect()
    }
}

#[cfg(test)]
mod tests;
