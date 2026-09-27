//! Starting a process in a pseudoterminal.
//!
//! The platform half of a pane. On Unix this is `portable-pty`. On Windows
//! it is Dispatch's own ConPTY spawn: a pane's process has to be created
//! suspended and put in a Job Object before it runs, or anything it starts
//! first escapes the job -- and `portable-pty` starts it running.
//!
//! On Unix a pane's process is kept unreaped from its exit until the pane is
//! closed. Its pid is its process group's id, which closing the pane ends the
//! group by, and a pid reaped at the exit could be another process's by then
//! (see [`Child`]).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows;

/// What to start.
#[derive(Debug, Clone, Copy)]
pub struct PtyCommand<'a> {
    /// The program, found on `PATH` when not a path itself.
    pub program: &'a str,
    /// Its arguments.
    pub args: &'a [String],
    /// Variables set on top of the environment this process has.
    pub env: &'a BTreeMap<String, String>,
    /// Variables it must not have at all, whether `env` or this process's
    /// environment would give them one.
    pub env_remove: &'a BTreeSet<String>,
    /// Where it starts.
    pub cwd: &'a Path,
}

/// A process running in a pseudoterminal.
pub struct PtyProcess {
    /// What the process prints.
    pub reader: Box<dyn Read + Send>,
    /// What it reads, as if typed.
    pub writer: Box<dyn Write + Send>,
    /// The pseudoterminal. Held for as long as the process should have one.
    pub terminal: Terminal,
    /// The process, for waiting on and for ending its tree.
    pub child: Child,
    /// Its process id.
    pub pid: Option<u32>,
}

impl std::fmt::Debug for PtyProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyProcess")
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// Whether a pseudoterminal's output has to be read to its end, even once
/// nobody wants it, for the pseudoterminal to finish closing.
///
/// On Windows it does: before Windows 11 24H2, closing a pseudoconsole waits
/// until what it still has to say has been read. On Unix a reader that kept
/// on would instead hold the terminal open for as long as anything holds its
/// other side -- a process that left the pane's session, say -- so there a
/// reader stops as soon as nobody wants what it reads.
pub const OUTPUT_OUTLIVES_ITS_READER: bool = cfg!(windows);

/// A pseudoterminal. Dropping it ends it.
pub struct Terminal(imp::Terminal);

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Terminal")
    }
}

impl Terminal {
    /// Resizes the pseudoterminal; the process learns of it as a terminal
    /// resize.
    pub fn resize(&self, rows: u16, cols: u16) -> std::io::Result<()> {
        self.0.resize(rows, cols)
    }
}

/// A pane's process, shared between whatever waits for it and whatever ends
/// it.
///
/// On Unix the process is not reaped when it exits. It is left a zombie
/// until [`Child::end_tree`], so its pid, which is also its process group's
/// id, cannot be given to another process first. Ending the group by that id
/// long after the exit then reaches only what the pane started. The cost is
/// one zombie per pane that has exited and not yet been closed.
pub struct Child(imp::Child);

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Child")
    }
}

impl Child {
    /// Waits for the process to exit, and says how, without letting its pid
    /// go.
    ///
    /// 0 for success, the process's code otherwise, and 1 for a failure
    /// that carried no code: a failed exit must never read as a clean one.
    /// Safe to call from one thread while another ends the tree.
    #[must_use]
    pub fn wait_exit(&self) -> i32 {
        self.0.wait_exit()
    }

    /// Ends the process and everything in its tree, then lets its pid go.
    ///
    /// Only the first call does anything: once the pid has been let go, a
    /// second ending by it could reach another process.
    pub fn end_tree(&self, grace: std::time::Duration) -> Result<(), crate::process::ProcessError> {
        self.0.end_tree(grace)
    }
}

/// Starts `command` in a new pseudoterminal of `rows` by `cols`.
///
/// A command with a NUL anywhere in it is refused with
/// [`std::io::ErrorKind::InvalidInput`] before anything starts.
pub fn spawn(command: &PtyCommand<'_>, rows: u16, cols: u16) -> std::io::Result<PtyProcess> {
    refuse_nul(command)?;
    imp::spawn(command, rows, cols)
}

/// Refuses a command with a NUL anywhere in it.
///
/// Everything reaches the system as NUL-terminated strings, so a NUL inside
/// one ends it early: on Windows, an argument would lose its tail and every
/// argument after it, and an environment variable every one after it,
/// without a word. `std::process::Command` refuses such a command, and
/// `portable-pty` refused a NUL in an argument; this is that refusal, made on
/// every platform before anything is started, saying which part held it.
fn refuse_nul(command: &PtyCommand<'_>) -> std::io::Result<()> {
    let refuse = |part: String| {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("the {part} holds a NUL, which would cut it short"),
        ))
    };

    if command.program.contains('\0') {
        return refuse("program".into());
    }
    if let Some(n) = command.args.iter().position(|arg| arg.contains('\0')) {
        return refuse(format!("argument {}", n + 1));
    }
    if let Some((key, _)) = command
        .env
        .iter()
        .find(|(key, value)| key.contains('\0') || value.contains('\0'))
    {
        return refuse(format!("environment variable {key:?}"));
    }
    if let Some(key) = command.env_remove.iter().find(|key| key.contains('\0')) {
        return refuse(format!("environment variable {key:?}"));
    }
    if command.cwd.as_os_str().as_encoded_bytes().contains(&0) {
        return refuse("working directory".into());
    }
    Ok(())
}

/// `arg` quoted so the C runtime's command-line parser splits it back out
/// unchanged.
///
/// Left alone when it has no space, tab, newline, vertical tab or quote;
/// otherwise wrapped in quotes, with each quote escaped and the backslashes
/// before a quote -- or before the closing quote -- doubled. The rules
/// `portable-pty` 0.9 followed, so every argument reaches a program as it
/// did. They are not `cmd.exe`'s rules: nothing that has to reach a program
/// intact can travel as an argument that `cmd.exe` parses again. That is why
/// a task for an agent behind `cmd.exe` goes to its standard input
/// (`dispatch_config::TaskInput::File`), and only `<%DISPATCH_TASK_FILE%`,
/// which this leaves bare, names it on the command line.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "only the Windows spawn builds a command line; the tests run everywhere"
    )
)]
fn quote_for_crt(arg: &str) -> String {
    let plain = !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']);
    if plain {
        return arg.to_string();
    }

    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
            continue;
        }
        let doubled = if c == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.extend(std::iter::repeat_n('\\', doubled));
        backslashes = 0;
        quoted.push(c);
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// A child's environment as the Windows spawn builds it: what it inherits,
/// with overrides on top and removals taken out, every name matched without
/// regard to case -- one variable, however it is spelled.
///
/// The one place names are folded. The spawn builds the child's
/// environment with it, and whoever judges what the child will run --
/// which file its `PATH` makes of a bare command -- reads it back through
/// the same fold, so the two can never see different values. Of overrides
/// spelled alike but for case, the one later in `BTreeMap` order wins and
/// keeps its spelling: `Path` over `PATH`.
///
/// Not behind a `#[cfg]`: the rule is Windows', but what is judged for
/// Windows is judged, and tested, everywhere.
#[derive(Debug, Clone, Default)]
pub struct WindowsEnvironment(BTreeMap<String, (OsString, OsString)>);

impl WindowsEnvironment {
    /// `inherited`, with `overrides` set on top and `removed` taken out.
    pub fn new(
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
        overrides: &BTreeMap<String, String>,
        removed: &BTreeSet<String>,
    ) -> Self {
        let mut variables: BTreeMap<String, (OsString, OsString)> = inherited
            .into_iter()
            .map(|(name, value)| (fold(&name.to_string_lossy()), (name, value)))
            .collect();
        for (name, value) in overrides {
            variables.insert(fold(name), (name.into(), value.into()));
        }
        for name in removed {
            variables.remove(&fold(name));
        }
        Self(variables)
    }

    /// The value of `name`, however either is spelled.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.0.get(&fold(name)).map(|(_, value)| value.as_os_str())
    }

    /// Every variable, once, as its winning spelling has it.
    pub fn variables(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.0
            .values()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }
}

/// A variable's name as Windows compares names.
fn fold(name: &str) -> String {
    name.to_uppercase()
}

/// Whether `a` and `b` name one environment variable here: compared as
/// [`WindowsEnvironment`] compares them on Windows, exactly elsewhere.
///
/// For whoever merges variables into a child's environment: two spellings
/// of one Windows variable left side by side are resolved by the spawn's
/// rule rather than the merger's intent.
#[must_use]
pub fn same_variable(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        fold(a) == fold(b)
    } else {
        a == b
    }
}

/// The file the Windows spawn starts for `program`, given the child's `PATH`
/// and `PATHEXT`.
///
/// A path is taken as it is, or with the first `PATHEXT` extension that
/// makes it name a file; a bare name is looked for the same way in each
/// `PATH` directory in turn; and what is found nowhere is returned as given,
/// for `CreateProcessW` to fail on. With no `PATHEXT`, only `.EXE` is tried.
///
/// Public, and not behind a `#[cfg]`, because what a command *is* matters
/// before it is started: `claude` found as `claude.cmd` is a batch file,
/// which Windows runs through `cmd.exe`, and whoever decides what may reach
/// its command line has to judge the file this finds, not the name it was
/// given. `portable-pty` 0.9 differed in two ways: it replaced an extension
/// the name already had rather than adding one, and it looked for a relative
/// path with a separator on `PATH`, where this finds it from the current
/// directory, as `CreateProcessW` does.
#[must_use]
pub fn resolve_program(program: &str, path: Option<&OsStr>, pathext: Option<&OsStr>) -> PathBuf {
    let pathext = pathext.map_or_else(|| ".EXE".into(), OsStr::to_string_lossy);

    let given = Path::new(program);
    if given.is_absolute() || given.components().count() > 1 {
        return find_with_pathext(given, &pathext).unwrap_or_else(|| given.to_path_buf());
    }

    if let Some(path) = path {
        for dir in std::env::split_paths(path) {
            if let Some(found) = find_with_pathext(&dir.join(program), &pathext) {
                return found;
            }
        }
    }

    given.to_path_buf()
}

/// `path` when it names a file, or else `path` with the first extension in
/// `pathext` -- a `;`-separated list, as the variable is -- that makes it
/// name one.
///
/// How Windows finds a program named without its extension: `claude` is run
/// as `claude.exe`, or as `claude.cmd` when that is what an installer left.
fn find_with_pathext(path: &Path, pathext: &str) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    pathext
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            let mut candidate = path.as_os_str().to_owned();
            candidate.push(extension);
            PathBuf::from(candidate)
        })
        .find(|candidate| candidate.is_file())
}

#[cfg(unix)]
mod imp {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

    use crate::process::{KILL_TIMEOUT, ProcessError, signal_group, wait_for_group_to_exit};

    pub(super) struct Terminal(Box<dyn MasterPty + Send>);

    impl Terminal {
        pub(super) fn resize(&self, rows: u16, cols: u16) -> std::io::Result<()> {
            self.0
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|error| std::io::Error::other(format!("{error:#}")))
        }
    }

    /// A pane's process, kept unreaped until its tree is ended.
    pub(super) struct Child {
        pid: libc::pid_t,
        /// `portable-pty`'s handle, through which the process is finally
        /// reaped. Taken when it is.
        handle: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
        /// How the process exited, once anything has seen it.
        exit: Mutex<Option<i32>>,
        /// Whether the tree has been ended.
        ended: AtomicBool,
    }

    impl Child {
        fn new(handle: Box<dyn portable_pty::Child + Send + Sync>) -> std::io::Result<Self> {
            let pid = handle
                .process_id()
                .and_then(|pid| libc::pid_t::try_from(pid).ok())
                .ok_or_else(|| std::io::Error::other("the pane's process has no pid"))?;
            Ok(Self {
                pid,
                handle: Mutex::new(Some(handle)),
                exit: Mutex::new(None),
                ended: AtomicBool::new(false),
            })
        }

        fn seen(&self) -> Option<i32> {
            *self.exit.lock().unwrap_or_else(|e| e.into_inner())
        }

        fn record(&self, code: i32) {
            *self.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
        }

        pub(super) fn wait_exit(&self) -> i32 {
            loop {
                if let Some(code) = self.seen() {
                    return code;
                }
                match wait_without_reaping(self.pid, true) {
                    Ok(Some(code)) => {
                        self.record(code);
                        return code;
                    }
                    // An answer that was not an exit. Asked again after a
                    // moment, so a kernel that keeps giving it for a stopped
                    // child is polled rather than spun on.
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
                    // Reaped under this wait by `end_tree`, which recorded
                    // the exit before it reaped.
                    Err(_) => return self.seen().unwrap_or(1),
                }
            }
        }

        /// Polls for the exit until `deadline`, without reaping. Returns
        /// whether the process has exited.
        fn exited_by(&self, deadline: Instant) -> bool {
            loop {
                if self.seen().is_some() {
                    return true;
                }
                if let Ok(Some(code)) = wait_without_reaping(self.pid, false) {
                    self.record(code);
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// Reaps the process, once it is known to have exited. One stuck in
        /// the kernel is left alone rather than waited on for good.
        fn reap(&self) {
            if self.seen().is_none() {
                return;
            }
            if let Some(mut handle) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = handle.wait();
            }
        }

        pub(super) fn end_tree(&self, grace: Duration) -> Result<(), ProcessError> {
            if self.ended.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            let pid = self.pid.unsigned_abs();
            let map = |source| ProcessError::Terminate { pid, source };
            let deadline = Instant::now() + grace;

            // The leader is unreaped, so its pid is still its group's id and
            // the group is certainly this pane's.
            signal_group(pid, libc::SIGTERM).map_err(map)?;
            if !self.exited_by(deadline) {
                signal_group(pid, libc::SIGKILL).map_err(map)?;
                self.exited_by(Instant::now() + KILL_TIMEOUT);
            }

            // Reaped before the group is waited on: an unreaped leader counts
            // as a member, and the wait would run out every time. From here
            // the group's id is held by whatever is left in it, and once a
            // probe finds it gone nothing signals it again. Two windows are
            // left, both needing the pid space to wrap within microseconds:
            // one between this reap and the first probe, when the leader was
            // the group's last member and the id is briefly unheld; the other
            // between a probe that found members and the kill straight after
            // it.
            self.reap();
            let rest = deadline.saturating_duration_since(Instant::now());
            if !wait_for_group_to_exit(pid, rest)
                && signal_group(pid, libc::SIGKILL).map_err(map)?
            {
                wait_for_group_to_exit(pid, KILL_TIMEOUT);
            }
            Ok(())
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            // A child whose tree was never ended is reaped if it has exited;
            // one still running when its last holder lets go is left
            // unreaped, because blocking a drop on it would be worse.
            if let Some(mut handle) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = handle.try_wait();
            }
        }
    }

    /// Waits for `pid` to exit without reaping it, or with `block` false
    /// looks once. Returns how it exited, once it has.
    pub(super) fn wait_without_reaping(
        pid: libc::pid_t,
        block: bool,
    ) -> std::io::Result<Option<i32>> {
        // SAFETY: an all-zero siginfo_t is a valid place for waitid to write.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let mut options = libc::WEXITED | libc::WNOWAIT;
        if !block {
            options |= libc::WNOHANG;
        }
        // SAFETY: P_PID with this process's own child's pid, and `info` is a
        // valid siginfo_t to write to.
        let result = unsafe { libc::waitid(libc::P_PID, pid.unsigned_abs(), &mut info, options) };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }

        // With WNOHANG and nothing to report, waitid leaves the pid zero.
        // SAFETY: waitid filled `info` in for a child event.
        let (who, code, status) = unsafe { (info.si_pid(), info.si_code, info.si_status()) };
        if who == 0 {
            return Ok(None);
        }
        // Only an exit is one. WEXITED asks for nothing else, but macOS has
        // been seen to answer for a child that has only stopped
        // (golang/go#19314), and a stopped leader taken for exited would be
        // reaped by a wait lasting as long as it stays stopped.
        Ok(match code {
            libc::CLD_EXITED => Some(status),
            libc::CLD_KILLED | libc::CLD_DUMPED => Some(1),
            _ => None,
        })
    }

    pub(super) fn spawn(
        command: &super::PtyCommand<'_>,
        rows: u16,
        cols: u16,
    ) -> std::io::Result<super::PtyProcess> {
        // `portable-pty` reports through `anyhow`; this crate speaks
        // `io::Error`, with the whole chain kept in the message.
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| {
                std::io::Error::other(format!("failed to open a pseudoterminal: {e:#}"))
            })?;

        let mut builder = CommandBuilder::new(command.program);
        builder.args(command.args);
        builder.cwd(command.cwd);
        for (key, value) in command.env {
            builder.env(key, value);
        }
        // After `env`, so a variable that is both set and removed is gone.
        for key in command.env_remove {
            builder.env_remove(key);
        }

        let child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| std::io::Error::other(format!("{e:#}")))?;

        // The slave is held open by the child, and dropping this copy is what
        // lets the reader see end-of-file when the child exits.
        drop(pair.slave);

        let pid = child.process_id();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(format!("{e:#}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| std::io::Error::other(format!("{e:#}")))?;

        Ok(super::PtyProcess {
            reader,
            writer,
            terminal: super::Terminal(Terminal(pair.master)),
            child: super::Child(Child::new(child)?),
            pid,
        })
    }
}

#[cfg(windows)]
use self::windows as imp;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::ErrorKind;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use super::{
        PtyCommand, WindowsEnvironment, find_with_pathext, quote_for_crt, refuse_nul,
        resolve_program, spawn,
    };

    /// A fresh directory holding an empty file for each of `names`.
    fn a_dir_with(label: &str, names: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dispatch-os-pathext-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir is writable");
        for name in names {
            std::fs::write(dir.join(name), b"").expect("temp dir is writable");
        }
        dir
    }

    #[test]
    fn a_program_path_is_completed_with_the_first_pathext_that_names_a_file() {
        const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";
        let dir = a_dir_with(
            "complete",
            &["claude.CMD", "tool.EXE", "tool.COM", "exact", "exact.EXE"],
        );
        std::fs::create_dir_all(dir.join("folder")).expect("temp dir is writable");
        std::fs::write(dir.join("folder.EXE"), b"").expect("temp dir is writable");

        assert_eq!(
            find_with_pathext(&dir.join("claude"), PATHEXT),
            Some(dir.join("claude.CMD")),
            "completed with the extension that names a file"
        );
        assert_eq!(
            find_with_pathext(&dir.join("tool"), PATHEXT),
            Some(dir.join("tool.COM")),
            "in PATHEXT's order"
        );
        assert_eq!(
            find_with_pathext(&dir.join("exact"), PATHEXT),
            Some(dir.join("exact")),
            "a file named exactly is taken as it is"
        );
        assert_eq!(
            find_with_pathext(&dir.join("folder"), PATHEXT),
            Some(dir.join("folder.EXE")),
            "a directory is not a program"
        );
        assert_eq!(find_with_pathext(&dir.join("missing"), PATHEXT), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_windows_environment_holds_one_variable_for_every_spelling() {
        use std::ffi::{OsStr, OsString};

        let inherited = [
            (OsString::from("Path"), OsString::from("inherited")),
            (OsString::from("TEMP"), OsString::from("scratch")),
            (OsString::from("Keep"), OsString::from("kept")),
        ];
        let overrides = BTreeMap::from([
            ("PATH".to_string(), "the daemon's".to_string()),
            ("Path".to_string(), "the harness's".to_string()),
            ("Extra".to_string(), "added".to_string()),
        ]);
        let removed = BTreeSet::from(["temp".to_string()]);

        let environment = WindowsEnvironment::new(inherited, &overrides, &removed);

        assert_eq!(
            environment.get("path"),
            Some(OsStr::new("the harness's")),
            "an override replaces what is inherited, and of two spellings the \
             later in order wins, as the spawn has always had it"
        );
        assert_eq!(environment.get("TEMP"), None, "removed whatever its case");
        assert_eq!(environment.get("KEEP"), Some(OsStr::new("kept")));
        assert_eq!(environment.get("extra"), Some(OsStr::new("added")));

        let names: Vec<_> = environment.variables().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            [OsStr::new("Extra"), OsStr::new("Keep"), OsStr::new("Path")],
            "one of each, spelled as the winner spelled it"
        );
    }

    #[test]
    fn a_bare_name_is_the_first_file_on_path_that_pathext_completes() {
        let first = a_dir_with("resolve-first", &["agent.CMD"]);
        let second = a_dir_with("resolve-second", &["agent.EXE", "other.EXE"]);
        let path = std::env::join_paths([&first, &second]).expect("the directories join");
        let pathext = std::ffi::OsStr::new(".EXE;.CMD");

        assert_eq!(
            resolve_program("agent", Some(&path), Some(pathext)),
            first.join("agent.CMD"),
            "the directory order decides, then the extension order"
        );
        assert_eq!(
            resolve_program("other", Some(&path), Some(pathext)),
            second.join("other.EXE")
        );
        assert_eq!(
            resolve_program("missing", Some(&path), Some(pathext)),
            std::path::PathBuf::from("missing"),
            "what is found nowhere is left for the start to fail on"
        );
        assert_eq!(
            resolve_program("agent", Some(&path), None),
            second.join("agent.EXE"),
            "without PATHEXT only .EXE is tried"
        );
        let given = first.join("agent");
        assert_eq!(
            resolve_program(&given.to_string_lossy(), None, Some(pathext)),
            first.join("agent.CMD"),
            "a path is completed where it points, whatever PATH says"
        );

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    #[test]
    fn a_nul_anywhere_in_a_command_is_refused() {
        let args = vec!["-c".to_string(), "echo hi".to_string()];
        let env = BTreeMap::from([("KEY".to_string(), "value".to_string())]);
        let none = BTreeSet::new();
        let cwd = std::env::temp_dir();
        let clean = PtyCommand {
            program: "sh",
            args: &args,
            env: &env,
            env_remove: &none,
            cwd: &cwd,
        };
        refuse_nul(&clean).expect("a command with no NUL in it is let through");

        let refused = |command: PtyCommand<'_>, part: &str| {
            let error = refuse_nul(&command).expect_err(part);
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{part}: {error}");
            assert!(
                error.to_string().contains(part),
                "{error:?} names no {part}"
            );
        };

        refused(
            PtyCommand {
                program: "s\0h",
                ..clean
            },
            "program",
        );
        let cut = vec!["-c".to_string(), "echo hi\0; echo more".to_string()];
        refused(
            PtyCommand {
                args: &cut,
                ..clean
            },
            "argument",
        );
        let key = BTreeMap::from([("K\0EY".to_string(), "value".to_string())]);
        refused(PtyCommand { env: &key, ..clean }, "environment");
        let value = BTreeMap::from([("KEY".to_string(), "val\0ue".to_string())]);
        refused(
            PtyCommand {
                env: &value,
                ..clean
            },
            "environment",
        );
        let removed = BTreeSet::from(["K\0EY".to_string()]);
        refused(
            PtyCommand {
                env_remove: &removed,
                ..clean
            },
            "environment",
        );
        let dir = cwd.join("a\0b");
        refused(PtyCommand { cwd: &dir, ..clean }, "directory");
    }

    #[test]
    fn a_command_with_a_nul_in_an_argument_is_not_started() {
        // Cut at the NUL, this would have run `exit 0` on Windows and said
        // nothing of what was lost.
        let (program, args) = if cfg!(windows) {
            (
                "cmd.exe",
                vec!["/c".to_string(), "exit 0\0& exit 1".to_string()],
            )
        } else {
            ("sh", vec!["-c".to_string(), "exit 0\0; exit 1".to_string()])
        };
        let env = BTreeMap::new();
        let cwd = std::env::temp_dir();

        let error = spawn(
            &PtyCommand {
                program,
                args: &args,
                env: &env,
                env_remove: &BTreeSet::new(),
                cwd: &cwd,
            },
            24,
            80,
        )
        .expect_err("a command cut short by a NUL is not started");
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error}");
    }

    #[test]
    fn a_plain_argument_is_left_alone() {
        assert_eq!(quote_for_crt("claude"), "claude");
        assert_eq!(
            quote_for_crt("<%DISPATCH_TASK_FILE%"),
            "<%DISPATCH_TASK_FILE%"
        );
    }

    #[test]
    fn an_argument_with_a_space_is_quoted() {
        assert_eq!(quote_for_crt("two words"), "\"two words\"");
        assert_eq!(quote_for_crt(""), "\"\"");
    }

    #[test]
    fn quotes_and_the_backslashes_before_them_are_escaped() {
        assert_eq!(quote_for_crt(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_for_crt(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote_for_crt(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quote_for_crt(r"C:\a\b c"), r#""C:\a\b c""#);
    }

    /// `sh -c script` in a pseudoterminal, nothing set or removed.
    #[cfg(unix)]
    fn sh(script: &str) -> super::PtyProcess {
        let args = vec!["-c".to_string(), script.to_string()];
        spawn(
            &PtyCommand {
                program: "sh",
                args: &args,
                env: &BTreeMap::new(),
                env_remove: &BTreeSet::new(),
                cwd: &std::env::temp_dir(),
            },
            24,
            80,
        )
        .expect("sh starts")
    }

    /// The first line `reader` prints, within five seconds.
    #[cfg(unix)]
    fn first_line(mut reader: Box<dyn std::io::Read + Send>) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            let mut byte = [0u8; 1];
            while reader.read(&mut byte).unwrap_or(0) == 1 {
                if byte[0] == b'\n' {
                    break;
                }
                seen.push(byte[0]);
            }
            let _ = tx.send(String::from_utf8_lossy(&seen).trim().to_string());
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("the script printed a line")
    }

    #[test]
    #[cfg(unix)]
    fn an_exited_leader_is_kept_unreaped_until_its_tree_is_ended() {
        // Reaped at once, its pid -- its process group's id -- would be free
        // for another process, and ending the group by it later would reach a
        // stranger's.
        let process = sh("exit 3");
        let pid = libc::pid_t::try_from(process.pid.expect("a pid")).expect("fits");

        assert_eq!(process.child.wait_exit(), 3);
        assert_eq!(
            super::imp::wait_without_reaping(pid, false).expect("still this process's child"),
            Some(3),
            "the leader is kept, unreaped"
        );

        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");
        assert!(
            super::imp::wait_without_reaping(pid, false).is_err(),
            "and reaped once its tree is ended"
        );
    }

    #[test]
    #[cfg(unix)]
    fn ending_an_exited_panes_tree_reaches_what_it_left_and_returns_promptly() {
        // The unreaped leader still counts as a member of its group. Were it
        // not reaped before the wait for the group, every close would wait out
        // the grace and the kill timeout too. A grace much longer than the 1 s
        // bound below is what makes that failure visible: reaping promptly
        // finishes in about 0.01 s regardless, but a version that reaps only
        // after waiting out the group would cost this whole grace, every time.
        let process = sh("trap '' HUP; sleep 30 & echo $!; exit 0");
        let left: u32 = first_line(process.reader)
            .parse()
            .expect("the script printed the sleep's pid");
        assert_eq!(process.child.wait_exit(), 0);
        assert!(
            crate::process::is_running(left),
            "the sleep outlived its shell"
        );

        let started = Instant::now();
        process
            .child
            .end_tree(Duration::from_secs(3))
            .expect("ending succeeds");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "ending took {:?}",
            started.elapsed()
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while crate::process::is_running(left) {
            assert!(Instant::now() < deadline, "what the pane left outlived it");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_leader_that_ignores_sigterm_is_killed_and_reaped() {
        let process = sh("trap '' TERM; echo ready; sleep 30");
        let pid = libc::pid_t::try_from(process.pid.expect("a pid")).expect("fits");
        assert_eq!(first_line(process.reader), "ready");

        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");

        assert!(
            super::imp::wait_without_reaping(pid, false).is_err(),
            "killed after the grace, and reaped"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_stopped_leader_is_not_taken_for_one_that_exited() {
        // macOS has been seen to answer a wait for exits for a child that has
        // only stopped (golang/go#19314). Taken for exited, the leader would
        // be reported so, and reaped by a wait lasting as long as it stays
        // stopped.
        let process = sh("echo ready; kill -STOP $$; exit 0");
        let pid = process.pid.expect("a pid");
        assert_eq!(first_line(process.reader), "ready");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .expect("ps runs");
            if String::from_utf8_lossy(&state.stdout)
                .trim()
                .starts_with('T')
            {
                break;
            }
            assert!(Instant::now() < deadline, "the shell never stopped");
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid = libc::pid_t::try_from(pid).expect("fits");
        assert_eq!(
            super::imp::wait_without_reaping(pid, false).expect("still this process's child"),
            None,
            "a stop is not an exit"
        );

        let started = Instant::now();
        process
            .child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "ending took {:?}",
            started.elapsed()
        );
        assert_ne!(
            process.child.wait_exit(),
            0,
            "a killed process never reads as a clean exit"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_waiter_parked_on_the_exit_hears_it_when_the_tree_is_ended() {
        // The pane's waiter thread is parked on the exit while a close ends
        // the tree and reaps the leader under it. It still hears how the
        // process ended, once.
        let process = sh("sleep 30");
        let child = std::sync::Arc::new(process.child);
        let waiter = {
            let child = std::sync::Arc::clone(&child);
            std::thread::spawn(move || child.wait_exit())
        };
        std::thread::sleep(Duration::from_millis(100));

        child
            .end_tree(Duration::from_millis(250))
            .expect("ending succeeds");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !waiter.is_finished() {
            assert!(Instant::now() < deadline, "the waiter never heard the exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(
            waiter.join().expect("the waiter finishes"),
            0,
            "a killed process never reads as a clean exit"
        );
    }
}
