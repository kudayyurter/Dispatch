//! Safe terminal emulation.
//!
//! All `unsafe` in Dispatch is confined to this module and [`crate::sys`].
//! Nothing above it touches a raw pointer, and no handle borrowed from
//! libghostty-vt escapes a call.

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::sys;

/// A failure reported by libghostty-vt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{operation} failed: {code}")]
pub struct VtError {
    /// The C function that failed.
    pub operation: &'static str,
    /// The `GhosttyResult` it returned.
    pub code: i32,
}

impl VtError {
    fn check(operation: &'static str, code: sys::GhosttyResult) -> Result<(), Self> {
        if code == sys::SUCCESS {
            Ok(())
        } else {
            Err(Self { operation, code })
        }
    }
}

/// Where the cursor sits, in cells, zero-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Column.
    pub x: u16,
    /// Row within the active area.
    pub y: u16,
}

/// Terminal size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Width in cells.
    pub cols: u16,
    /// Height in cells.
    pub rows: u16,
}

impl Size {
    /// Creates a size, clamping each dimension to at least one cell.
    ///
    /// libghostty-vt rejects a zero dimension, and a terminal briefly sized to
    /// nothing is a normal consequence of a window being dragged small.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }
}

/// Where the viewport is in the scrollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    /// Rows in the whole scrollable area.
    pub total: u64,
    /// The first row the viewport shows.
    pub offset: u64,
    /// Rows the viewport shows.
    pub len: u64,
}

impl Scrollbar {
    /// How many rows of newer output lie below the viewport.
    #[must_use]
    pub fn above_live(&self) -> u64 {
        self.total
            .saturating_sub(self.offset)
            .saturating_sub(self.len)
    }
}

/// Where to move a pane's viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTo {
    /// The oldest output still held.
    Top,
    /// The active area, where new output appears.
    Bottom,
    /// A signed number of rows. Negative moves towards older output.
    Delta(isize),
}

/// What an answering terminal's callbacks write to while it is fed.
///
/// Boxed, so its address stays put for as long as libghostty-vt holds it as
/// userdata, however the `VtTerminal` that owns it moves.
#[derive(Debug)]
struct Answers {
    /// Replies not yet taken.
    replies: Vec<u8>,
    /// The size to report, kept in step with every resize.
    size: Size,
}

/// The terminal state behind one pane.
///
/// Feed it bytes from a pseudoterminal; read a screen back out.
///
/// Swapping this for another VT engine means implementing this surface and
/// nothing else.
#[derive(Debug)]
pub struct VtTerminal {
    /// Owned handle. Freed exactly once, in `Drop`.
    handle: sys::Terminal,
    /// The userdata the handle's callbacks point at, when this terminal
    /// answers; from `Box::leak`, reclaimed in `Drop`.
    ///
    /// A raw pointer rather than a `Box` on purpose: the library holds a copy
    /// of it, and touching a live `Box` through its own unique tag would
    /// invalidate that copy. Every Rust-side access goes through this pointer
    /// too, so the library's copy and ours stay in one provenance chain.
    answers: Option<NonNull<Answers>>,
}

// SAFETY: the handle is owned exclusively by this value and libghostty-vt
// does not use thread-local state for it. The handle also holds a pointer to
// the heap `Answers` this value owns, which moves with it and is touched only
// through `&mut self` (or from callbacks that run inside such a call).
// `&mut self` on every mutating method keeps the library's no-reentrancy
// requirement for vt_write.
unsafe impl Send for VtTerminal {}

impl VtTerminal {
    /// Creates a terminal of the given size.
    pub fn new(size: Size) -> Result<Self, VtError> {
        let mut handle: sys::Terminal = std::ptr::null_mut();

        // SAFETY: `handle` is a valid out-pointer, and a null allocator
        // selects the library's default allocator per allocator.h.
        let code = unsafe {
            sys::ghostty_terminal_new(std::ptr::null(), &raw mut handle, size.cols, size.rows)
        };
        VtError::check("ghostty_terminal_new", code)?;

        debug_assert!(!handle.is_null(), "success must yield a handle");
        Ok(Self {
            handle,
            answers: None,
        })
    }

    /// Creates a terminal that answers the questions programs ask it:
    /// device attributes, cursor position, size, mode reports.
    ///
    /// Only one emulator per pane may answer, or a program would get every
    /// answer twice. A window drawing a pane it does not own uses
    /// [`VtTerminal::new`], which answers nothing.
    pub fn answering(size: Size) -> Result<Self, VtError> {
        let mut terminal = Self::new(size)?;
        let answers = NonNull::from(Box::leak(Box::new(Answers {
            replies: Vec::new(),
            size,
        })));

        // Installed before the library is told about it, so that `Drop`
        // frees it in the right order on every path, including a `set`
        // failing after the userdata is in place.
        terminal.answers = Some(answers);

        // SAFETY: the handle is live; userdata is the leaked box, which this
        // terminal frees only after the handle (see `Drop`); the callbacks
        // match the C signatures in `terminal.h`, and a callback option takes
        // the function pointer itself as its value.
        unsafe {
            VtError::check(
                "ghostty_terminal_set(USERDATA)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_USERDATA,
                    answers.as_ptr().cast(),
                ),
            )?;
            VtError::check(
                "ghostty_terminal_set(WRITE_PTY)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_WRITE_PTY,
                    write_pty as sys::WritePtyFn as *const c_void,
                ),
            )?;
            VtError::check(
                "ghostty_terminal_set(SIZE)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_SIZE,
                    report_size as sys::SizeFn as *const c_void,
                ),
            )?;
            VtError::check(
                "ghostty_terminal_set(XTVERSION)",
                sys::ghostty_terminal_set(
                    terminal.handle,
                    sys::OPT_XTVERSION,
                    report_version as sys::XtversionFn as *const c_void,
                ),
            )?;
        }

        // Dispatch draws no images, so the kitty graphics protocol is switched
        // off: a program that asks whether it is supported hears nothing and
        // falls back to something Dispatch can show, and the answerer never
        // stores images it would only hold in memory. This matches
        // `HOST_TERMINAL` in `session.rs`, which keeps Ghostty and kitty out
        // of the child's environment for the same reason.
        let no_images: u64 = 0;
        // SAFETY: the handle is live, and this option takes a `const
        // uint64_t*`, which `no_images` outlives for the call.
        let code = unsafe {
            sys::ghostty_terminal_set(
                terminal.handle,
                sys::OPT_KITTY_IMAGE_STORAGE_LIMIT,
                (&raw const no_images).cast(),
            )
        };
        VtError::check("ghostty_terminal_set(KITTY_IMAGE_STORAGE_LIMIT)", code)?;

        Ok(terminal)
    }

    /// The replies produced since the last call, to be written to the
    /// program's input. Always empty for a terminal made with
    /// [`VtTerminal::new`].
    pub fn take_replies(&mut self) -> Vec<u8> {
        let Some(answers) = self.answers else {
            return Vec::new();
        };
        // SAFETY: the pointer is the live allocation `answering` made; `&mut
        // self` means no feed or resize is running, so no callback holds a
        // reference to it.
        std::mem::take(unsafe { &mut (*answers.as_ptr()).replies })
    }

    /// Keeps no scrollback: for a terminal that only answers, which needs the
    /// screen's cursor and modes but never its history.
    pub fn disable_scrollback(&mut self) -> Result<(), VtError> {
        let zero: usize = 0;
        // SAFETY: the handle is live, and this option takes a `const size_t*`,
        // which `zero` outlives for the call.
        let code = unsafe {
            sys::ghostty_terminal_set(
                self.handle,
                sys::OPT_SCROLLBACK_MAX_BYTES,
                (&raw const zero).cast(),
            )
        };
        VtError::check("ghostty_terminal_set(SCROLLBACK_MAX_BYTES)", code)
    }

    /// The raw handle, for other modules in this crate that call the library.
    ///
    /// Crate-internal on purpose: the handle must not outlive `self`, and
    /// nothing outside this crate touches raw pointers.
    pub(crate) fn handle(&self) -> sys::Terminal {
        self.handle
    }

    /// Feeds bytes from the pseudoterminal into the emulator.
    ///
    /// Takes `&mut self` because libghostty-vt documents `vt_write` as
    /// non-reentrant.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // SAFETY: the handle is live for the lifetime of self, and `bytes`
        // outlives the call with its true length.
        unsafe { sys::ghostty_terminal_vt_write(self.handle, bytes.as_ptr(), bytes.len()) };
    }

    /// Resizes the terminal.
    ///
    /// Pixel dimensions are reported to programs that ask for them; zero means
    /// unknown, which is what a terminal that does not render glyphs itself
    /// should say.
    ///
    /// With in-band size reports (mode 2048) on, the library answers a resize
    /// from inside this very call, so the size the callback reports is
    /// updated first and put back if the resize fails.
    pub fn resize(&mut self, size: Size) -> Result<(), VtError> {
        let previous = self.answers.map(|answers| {
            // SAFETY: the live allocation from `answering`; `&mut self`
            // means no callback is running, and no reference outlives this
            // statement.
            unsafe { std::mem::replace(&mut (*answers.as_ptr()).size, size) }
        });

        // SAFETY: the handle is live and the size is non-zero in both
        // dimensions by construction. Callbacks may run inside this call;
        // they only touch `Answers` through the pointer, and nothing here
        // holds a reference to it.
        let code = unsafe { sys::ghostty_terminal_resize(self.handle, size.cols, size.rows, 0, 0) };
        if let (Some(answers), Some(previous), true) =
            (self.answers, previous, code != sys::SUCCESS)
        {
            // SAFETY: as above; the call has returned.
            unsafe { (*answers.as_ptr()).size = previous };
        }
        VtError::check("ghostty_terminal_resize", code)
    }

    /// Reads one `uint16_t`-valued field.
    fn get_u16(&self, selector: i32, operation: &'static str) -> Result<u16, VtError> {
        let mut value: u16 = 0;

        // SAFETY: every selector passed here is documented with output type
        // `uint16_t *`, which is what `value` is.
        let code = unsafe {
            sys::ghostty_terminal_get(self.handle, selector, (&raw mut value).cast::<c_void>())
        };
        VtError::check(operation, code)?;
        Ok(value)
    }

    /// The terminal's current size in cells.
    pub fn size(&self) -> Result<Size, VtError> {
        Ok(Size {
            cols: self.get_u16(sys::data::COLS, "ghostty_terminal_get(COLS)")?,
            rows: self.get_u16(sys::data::ROWS, "ghostty_terminal_get(ROWS)")?,
        })
    }

    /// Where the cursor sits.
    pub fn cursor(&self) -> Result<Cursor, VtError> {
        Ok(Cursor {
            x: self.get_u16(sys::data::CURSOR_X, "ghostty_terminal_get(CURSOR_X)")?,
            y: self.get_u16(sys::data::CURSOR_Y, "ghostty_terminal_get(CURSOR_Y)")?,
        })
    }

    /// Whether the child has turned bracketed paste on.
    ///
    /// Pasted text may only be wrapped in `\x1b[200~`/`\x1b[201~` when this is
    /// true. A child that never asked for the mode has no idea what those bytes
    /// mean and runs them as input: `sh` reads the wrapper as the start of a
    /// command and answers `00~…: command not found`.
    ///
    /// A failed query reads as off, which is the safe way round — the wrapper is
    /// an optimisation for children that understand it, and text arriving
    /// unwrapped is merely typing.
    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        let mut config = sys::ModeConfig {
            mode: sys::MODE_BRACKETED_PASTE,
            value: false,
        };

        // SAFETY: the terminal is live, and `config` is the type the MODE
        // selector documents, with its `mode` field set as the header requires.
        let result = unsafe {
            sys::ghostty_terminal_get(
                self.handle,
                sys::data::MODE,
                std::ptr::from_mut(&mut config).cast(),
            )
        };

        result == sys::SUCCESS && config.value
    }

    /// Moves the viewport over the scrollback.
    ///
    /// Scrolling is a property of the viewport, not of the screen contents, so
    /// output continues to arrive while scrolled back; it simply lands below
    /// what is being looked at.
    ///
    /// Has no effect on the alternate screen, which has no scrollback. A
    /// full-screen agent is therefore unaffected, which is correct: its own
    /// interface owns the whole viewport.
    pub fn scroll(&mut self, to: ScrollTo) {
        let behavior = match to {
            ScrollTo::Top => sys::ScrollViewport {
                tag: sys::scroll::TOP,
                value: sys::ScrollValue { _padding: [0; 2] },
            },
            ScrollTo::Bottom => sys::ScrollViewport {
                tag: sys::scroll::BOTTOM,
                value: sys::ScrollValue { _padding: [0; 2] },
            },
            ScrollTo::Delta(rows) => sys::ScrollViewport {
                tag: sys::scroll::DELTA,
                value: sys::ScrollValue { delta: rows },
            },
        };

        // SAFETY: the handle is live and the tag matches the union member set
        // above, which is the contract the tagged union documents.
        unsafe { sys::ghostty_terminal_scroll_viewport(self.handle, behavior) };
    }

    /// Where the viewport is in the scrollback, for saying how far back it is.
    pub fn scrollbar(&self) -> Result<Scrollbar, VtError> {
        let mut raw = sys::TerminalScrollbar::default();
        // SAFETY: the terminal is live, and `raw` is the type the SCROLLBAR
        // selector documents.
        let code = unsafe {
            sys::ghostty_terminal_get(
                self.handle,
                sys::data::SCROLLBAR,
                (&raw mut raw).cast::<c_void>(),
            )
        };
        VtError::check("ghostty_terminal_get(SCROLLBAR)", code)?;
        Ok(Scrollbar {
            total: raw.total,
            offset: raw.offset,
            len: raw.len,
        })
    }

    /// The visible screen as plain text, one line per row.
    ///
    /// Styling is dropped. Used for assertions and diagnostics; rendering
    /// reads cells and their styles instead.
    pub fn plain_text(&self) -> Result<String, VtError> {
        let mut formatter: sys::Formatter = std::ptr::null_mut();
        let options = sys::FormatterTerminalOptions::plain_text();

        // SAFETY: the handle outlives the formatter, which is freed below on
        // every path. A null allocator selects the default allocator.
        let code = unsafe {
            sys::ghostty_formatter_terminal_new(
                std::ptr::null(),
                &raw mut formatter,
                self.handle,
                options,
            )
        };
        VtError::check("ghostty_formatter_terminal_new", code)?;

        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len: usize = 0;

        // SAFETY: `formatter` is live, and both out-pointers are valid.
        let code = unsafe {
            sys::ghostty_formatter_format_alloc(
                formatter,
                std::ptr::null(),
                &raw mut ptr,
                &raw mut len,
            )
        };

        // Free the formatter before returning either way: the error path must
        // not leak it.
        //
        // SAFETY: `formatter` came from ghostty_formatter_terminal_new and is
        // not used afterwards.
        unsafe { sys::ghostty_formatter_free(formatter) };

        VtError::check("ghostty_formatter_format_alloc", code)?;

        if ptr.is_null() || len == 0 {
            return Ok(String::new());
        }

        // SAFETY: the library returned `ptr` with `len` initialised bytes,
        // and the slice is copied before the buffer is released.
        let text = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();

        // SAFETY: released with the same (default) allocator and length the
        // library allocated it with, as ghostty_formatter_format_alloc
        // documents.
        unsafe { sys::ghostty_free(std::ptr::null(), ptr, len) };

        // The formatter emits UTF-8; a replacement character is a better
        // outcome than refusing to render a pane over one bad byte.
        Ok(String::from_utf8_lossy(&text).into_owned())
    }
}

impl Drop for VtTerminal {
    fn drop(&mut self) {
        // SAFETY: the handle came from ghostty_terminal_new, is freed exactly
        // once here, and is not used afterwards.
        unsafe { sys::ghostty_terminal_free(self.handle) };

        // Strictly after the handle: it is the only thing that could still
        // call back into this allocation. Field declaration order does not
        // matter; this order does.
        if let Some(answers) = self.answers.take() {
            // SAFETY: from `Box::leak` in
            // `answering`, reclaimed exactly once, and the handle that held
            // it is gone.
            drop(unsafe { Box::from_raw(answers.as_ptr()) });
        }
    }
}

/// Collects a reply the terminal writes back to the program.
///
/// Called synchronously from inside `ghostty_terminal_vt_write`, and also from
/// inside `ghostty_terminal_resize` (with mode 2048 on, a resize writes its
/// report directly). Both run under `&mut VtTerminal`, which holds no
/// reference to the `Answers` meanwhile, so the callback is the only accessor.
/// It only copies bytes, and must never touch the terminal itself.
unsafe extern "C" fn write_pty(
    _terminal: sys::Terminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    // SAFETY: userdata is the `Answers` allocation installed by `answering`,
    // alive for as long as the handle; nothing else borrows it during a feed
    // or a resize. The
    // library guarantees `data` is valid for `len` bytes during the call.
    let (answers, bytes) = unsafe {
        (
            &mut *userdata.cast::<Answers>(),
            std::slice::from_raw_parts(data, len),
        )
    };
    answers.replies.extend_from_slice(bytes);
}

/// What XTVERSION reports: Dispatch, not the library it is built on.
///
/// Programs pick features by the terminal's name, and libghostty's default
/// would have them assume Ghostty's, kitty graphics among them.
const XTVERSION: &str = concat!("dispatch ", env!("CARGO_PKG_VERSION"));

/// Names the terminal for XTVERSION.
///
/// Returns a `'static` string, so the memory outlives the call as the library
/// requires; touches neither the userdata nor the terminal.
unsafe extern "C" fn report_version(
    _terminal: sys::Terminal,
    _userdata: *mut c_void,
) -> sys::GhosttyString {
    sys::GhosttyString {
        ptr: XTVERSION.as_ptr(),
        len: XTVERSION.len(),
    }
}

/// Reports the size in cells; zero pixels, since nothing here renders glyphs.
unsafe extern "C" fn report_size(
    _terminal: sys::Terminal,
    userdata: *mut c_void,
    out: *mut sys::SizeReportSize,
) -> bool {
    if userdata.is_null() || out.is_null() {
        return false;
    }
    // SAFETY: as in `write_pty`; `out` is a valid out-pointer for the call.
    unsafe {
        let answers = &*userdata.cast::<Answers>();
        *out = sys::SizeReportSize {
            rows: answers.size.rows,
            columns: answers.size.cols,
            cell_width: 0,
            cell_height: 0,
        };
    }
    true
}

/// Encodes pasted text for writing to a child.
///
/// Wraps it in bracketed paste markers when `bracketed`, turns newlines into
/// carriage returns when not — a child that cannot be told "this is a paste"
/// must at least be sent what a keyboard would send — and replaces the control
/// bytes that could otherwise end the paste and inject a command.
///
/// This is the vendored terminal library's own encoder rather than our reading
/// of the rules. Dispatch wrapped every paste by hand once, for children that
/// had not asked for the mode, and they ran the wrapper as a command.
#[must_use]
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    // The encoder rewrites its input, so it gets a copy rather than the
    // caller's string.
    let mut data = text.as_bytes().to_vec();

    // Enough for the markers and any expansion; a short paste is one call.
    let mut buf = vec![0u8; data.len() + 16];
    let mut written = 0usize;

    // SAFETY: both pointers address their own buffers for the lengths given,
    // and `written` is a live `usize`.
    let mut result = unsafe {
        sys::ghostty_paste_encode(
            data.as_mut_ptr(),
            data.len(),
            bracketed,
            buf.as_mut_ptr(),
            buf.len(),
            &raw mut written,
        )
    };

    if result == sys::OUT_OF_SPACE {
        // `written` now holds the size it wants, so the retry cannot fail for
        // the same reason.
        buf = vec![0u8; written];
        let mut data = text.as_bytes().to_vec();

        // SAFETY: as above, with a buffer the encoder has asked for by size.
        result = unsafe {
            sys::ghostty_paste_encode(
                data.as_mut_ptr(),
                data.len(),
                bracketed,
                buf.as_mut_ptr(),
                buf.len(),
                &raw mut written,
            )
        };
    }

    if result != sys::SUCCESS {
        return Vec::new();
    }

    buf.truncate(written);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> VtTerminal {
        VtTerminal::new(Size::new(80, 24)).expect("a terminal can be created")
    }

    /// Trims each line and drops trailing blank lines, so assertions can name
    /// the content without encoding the blank remainder of the screen.
    fn visible_lines(terminal: &VtTerminal) -> Vec<String> {
        let text = terminal.plain_text().expect("formatting succeeds");
        let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }

    #[test]
    fn an_unbracketed_paste_carries_no_markers() {
        // The bug this exists to stop: `sh` never enables mode 2004, so a
        // wrapper reaches it as text and it runs `00~` as a command.
        let encoded = encode_paste("one\ntwo", false);
        let text = String::from_utf8_lossy(&encoded);

        assert!(!text.contains("200~"), "got {text:?}");
        assert!(!text.contains("201~"), "got {text:?}");
        assert!(text.contains("one"), "got {text:?}");
        assert!(text.contains("two"), "got {text:?}");
    }

    #[test]
    fn a_bracketed_paste_is_wrapped_for_a_child_that_asked() {
        // An agent that turned the mode on wants the whole paste in one piece
        // rather than a line at a time.
        let encoded = encode_paste("one\ntwo", true);
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.starts_with("\x1b[200~"), "got {text:?}");
        assert!(text.ends_with("\x1b[201~"), "got {text:?}");
    }

    #[test]
    fn bracketed_paste_is_off_until_the_child_asks_for_it() {
        // Wrapping a paste for a child that never asked is how `sh` ends up
        // running `00~` as a command.
        let mut terminal = terminal();
        assert!(!terminal.bracketed_paste(), "nothing has enabled it");

        terminal.feed(b"\x1b[?2004h");
        assert!(terminal.bracketed_paste(), "the child enabled mode 2004");

        terminal.feed(b"\x1b[?2004l");
        assert!(!terminal.bracketed_paste(), "the child turned it off again");
    }

    #[test]
    fn plain_text_comes_back_out() {
        let mut terminal = terminal();
        terminal.feed(b"hello world");
        assert_eq!(visible_lines(&terminal), vec!["hello world"]);
    }

    #[test]
    fn styling_is_parsed_and_not_shown_as_text() {
        let mut terminal = terminal();
        terminal.feed(b"\x1b[31mred\x1b[0m plain");
        assert_eq!(visible_lines(&terminal), vec!["red plain"]);
    }

    #[test]
    fn the_cursor_moves_with_the_text() {
        let mut terminal = terminal();
        assert_eq!(terminal.cursor().expect("readable"), Cursor { x: 0, y: 0 });

        terminal.feed(b"abc");
        assert_eq!(terminal.cursor().expect("readable"), Cursor { x: 3, y: 0 });
    }

    #[test]
    fn absolute_cursor_positioning_is_honoured() {
        let mut terminal = terminal();
        // CUP is one-indexed; row 3 column 5 is (4, 2) in zero-indexed cells.
        terminal.feed(b"\x1b[3;5H");
        assert_eq!(terminal.cursor().expect("readable"), Cursor { x: 4, y: 2 });
    }

    #[test]
    fn newlines_advance_the_row() {
        let mut terminal = terminal();
        terminal.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(visible_lines(&terminal), vec!["one", "two", "three"]);
    }

    #[test]
    fn the_screen_can_be_erased() {
        let mut terminal = terminal();
        terminal.feed(b"visible");
        assert!(!visible_lines(&terminal).is_empty());

        terminal.feed(b"\x1b[2J");
        assert!(
            visible_lines(&terminal).is_empty(),
            "erase-in-display should clear the screen"
        );
    }

    #[test]
    fn the_reported_size_matches_what_was_asked_for() {
        let terminal = VtTerminal::new(Size::new(120, 40)).expect("creatable");
        assert_eq!(
            terminal.size().expect("readable"),
            Size {
                cols: 120,
                rows: 40
            }
        );
    }

    #[test]
    fn resizing_changes_the_reported_size() {
        let mut terminal = terminal();
        terminal.resize(Size::new(100, 30)).expect("resizable");
        assert_eq!(
            terminal.size().expect("readable"),
            Size {
                cols: 100,
                rows: 30
            }
        );
    }

    #[test]
    fn a_zero_dimension_is_clamped_rather_than_rejected() {
        // A window dragged to nothing should not take a pane down with it.
        let terminal = VtTerminal::new(Size::new(0, 0)).expect("zero is clamped, not rejected");
        assert_eq!(
            terminal.size().expect("readable"),
            Size { cols: 1, rows: 1 }
        );
    }

    #[test]
    fn text_survives_a_resize() {
        let mut terminal = terminal();
        terminal.feed(b"persistent");
        terminal.resize(Size::new(100, 30)).expect("resizable");
        assert_eq!(visible_lines(&terminal), vec!["persistent"]);
    }

    #[test]
    fn feeding_nothing_is_harmless() {
        let mut terminal = terminal();
        terminal.feed(b"");
        assert!(visible_lines(&terminal).is_empty());
    }

    #[test]
    fn a_split_escape_sequence_is_reassembled() {
        // A pseudoterminal read can end mid-sequence, so the emulator has to
        // carry parser state between feeds.
        let mut terminal = terminal();
        terminal.feed(b"\x1b[3");
        terminal.feed(b";5H");
        assert_eq!(terminal.cursor().expect("readable"), Cursor { x: 4, y: 2 });
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        let mut terminal = terminal();
        terminal.feed(&[0xff, 0xfe, b'o', b'k']);
        let _ = terminal.plain_text().expect("formatting still succeeds");
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    /// A terminal with more output than fits, so there is scrollback.
    fn scrolled() -> VtTerminal {
        let mut terminal = VtTerminal::new(Size::new(20, 5)).expect("creatable");
        for i in 1..=20 {
            terminal.feed(format!("line{i}\r\n").as_bytes());
        }
        terminal
    }

    fn lines(terminal: &VtTerminal) -> Vec<String> {
        terminal
            .plain_text()
            .expect("formatting succeeds")
            .lines()
            .map(|l| l.trim_end().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn the_viewport_starts_at_the_newest_output() {
        let terminal = scrolled();
        assert!(
            lines(&terminal).iter().any(|l| l.contains("line20")),
            "the newest line should be visible"
        );
    }

    #[test]
    fn scrolling_up_reveals_older_output() {
        let mut terminal = scrolled();
        terminal.scroll(ScrollTo::Delta(-10));

        let visible = lines(&terminal);
        assert!(
            visible.iter().any(|l| l.contains("line1"))
                || visible.iter().any(|l| l.contains("line5")),
            "older output should be visible, got {visible:?}"
        );
    }

    #[test]
    fn scrolling_to_the_top_shows_the_oldest_output() {
        let mut terminal = scrolled();
        terminal.scroll(ScrollTo::Top);

        assert!(
            lines(&terminal).iter().any(|l| l.contains("line1")),
            "the oldest line should be visible"
        );
    }

    #[test]
    fn scrolling_back_to_the_bottom_returns_to_the_newest() {
        let mut terminal = scrolled();
        terminal.scroll(ScrollTo::Top);
        terminal.scroll(ScrollTo::Bottom);

        assert!(
            lines(&terminal).iter().any(|l| l.contains("line20")),
            "returning to the bottom should show the newest line"
        );
    }

    #[test]
    fn output_keeps_arriving_while_scrolled_back() {
        // Scrolling is a property of the viewport, so an agent does not stop
        // working because someone is reading its history.
        let mut terminal = scrolled();
        terminal.scroll(ScrollTo::Top);
        terminal.feed(b"arrived-later\r\n");

        terminal.scroll(ScrollTo::Bottom);
        assert!(
            lines(&terminal).iter().any(|l| l.contains("arrived-later")),
            "output written while scrolled back should still be there"
        );
    }

    #[test]
    fn scrolling_past_the_ends_clamps_rather_than_panicking() {
        let mut terminal = scrolled();

        terminal.scroll(ScrollTo::Delta(-10_000));
        assert!(!lines(&terminal).is_empty(), "scrolled far up");

        terminal.scroll(ScrollTo::Delta(10_000));
        assert!(
            lines(&terminal).iter().any(|l| l.contains("line20")),
            "scrolled far down"
        );
    }

    #[test]
    fn scrolling_a_terminal_with_no_scrollback_is_harmless() {
        let mut terminal = VtTerminal::new(Size::new(20, 5)).expect("creatable");
        terminal.feed(b"only");

        terminal.scroll(ScrollTo::Top);
        terminal.scroll(ScrollTo::Delta(-5));

        assert!(lines(&terminal).iter().any(|l| l.contains("only")));
    }

    #[test]
    fn the_scrollbar_counts_the_lines_above_live() {
        let mut terminal = VtTerminal::new(Size::new(20, 5)).expect("a terminal");
        for line in 0..30 {
            terminal.feed(format!("line {line}\r\n").as_bytes());
        }
        let at_bottom = terminal.scrollbar().expect("a scrollbar");
        assert_eq!(at_bottom.above_live(), 0);
        assert_eq!(at_bottom.len, 5);
        assert!(at_bottom.total > 5);

        terminal.scroll(ScrollTo::Delta(-3));
        assert_eq!(terminal.scrollbar().expect("a scrollbar").above_live(), 3);
    }
}

#[cfg(test)]
mod answer_tests {
    use super::*;

    fn answering() -> VtTerminal {
        VtTerminal::answering(Size::new(80, 24)).expect("a terminal")
    }

    #[test]
    fn primary_device_attributes_are_answered() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[c");
        assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
    }

    #[test]
    fn a_drawing_terminal_answers_nothing() {
        let mut terminal = VtTerminal::new(Size::new(80, 24)).expect("a terminal");
        terminal.feed(b"\x1b[c\x1b[6n\x1b[18t");
        assert!(terminal.take_replies().is_empty());
    }

    #[test]
    fn replies_are_taken_once() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[c");
        assert!(!terminal.take_replies().is_empty());
        assert!(terminal.take_replies().is_empty());
    }

    #[test]
    fn the_cursor_position_is_reported_where_the_cursor_is() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[5;10H\x1b[6n");
        assert_eq!(terminal.take_replies(), b"\x1b[5;10R");
    }

    #[test]
    fn the_alternate_screen_reports_its_own_cursor() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[10;10H\x1b[?1049h\x1b[3;4H\x1b[6n");
        assert_eq!(terminal.take_replies(), b"\x1b[3;4R");
    }

    #[test]
    fn the_size_is_reported_in_cells() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[18t");
        assert_eq!(terminal.take_replies(), b"\x1b[8;24;80t");
    }

    #[test]
    fn a_resize_changes_the_reported_size() {
        let mut terminal = answering();
        terminal.resize(Size::new(100, 30)).expect("resizes");
        terminal.feed(b"\x1b[18t");
        assert_eq!(terminal.take_replies(), b"\x1b[8;30;100t");
    }

    #[test]
    fn a_query_split_across_two_feeds_is_answered() {
        let mut terminal = answering();
        terminal.feed(b"hello \x1b");
        terminal.feed(b"[c");
        assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
    }

    #[test]
    fn a_mode_query_is_answered() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[?2004h\x1b[?2004$p");
        assert_eq!(terminal.take_replies(), b"\x1b[?2004;1$y");
    }

    #[test]
    fn a_resize_reports_in_band_with_zero_pixels() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[?2048h");
        let _ = terminal.take_replies();
        terminal.resize(Size::new(100, 30)).expect("resizes");
        assert_eq!(terminal.take_replies(), b"\x1b[48;30;100;0;0t");
    }

    #[test]
    fn the_text_area_in_pixels_is_reported_as_unknown() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[14t");
        assert_eq!(terminal.take_replies(), b"\x1b[4;0;0t");
    }

    #[test]
    fn an_answerer_without_scrollback_still_answers() {
        let mut terminal = answering();
        terminal.disable_scrollback().expect("sets");
        for _ in 0..200 {
            terminal.feed(b"a line of output\r\n");
        }
        terminal.feed(b"\x1b[c");
        assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
    }

    #[test]
    fn a_kitty_graphics_query_goes_unanswered() {
        let mut terminal = answering();
        terminal.feed(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
        assert!(terminal.take_replies().is_empty());
        terminal.feed(b"\x1b[c");
        assert_eq!(terminal.take_replies(), b"\x1b[?62;22c");
    }

    #[test]
    fn xtversion_names_dispatch() {
        let mut terminal = answering();
        terminal.feed(b"\x1b[>q");
        let reply = String::from_utf8(terminal.take_replies()).expect("utf-8");
        assert_eq!(
            reply,
            concat!("\x1bP>|dispatch ", env!("CARGO_PKG_VERSION"), "\x1b\\")
        );
    }
}
