//! Drives the real `dispatch delegate` shim against a real `dispatchd`.
//!
//! `dispatchd`'s own integration tests cover the approved path end to end
//! (`a_delegate_caller_and_an_interface_client_share_one_daemon`); what they
//! cannot cover is the shim itself, since `CARGO_BIN_EXE_dispatch` is only
//! defined for the package under test. This file lives in `dispatch`'s own
//! test suite for exactly that reason, and covers the two paths through the
//! shim's exit-code mapping that the approved path never exercises: a human
//! saying no, and the daemon refusing before anybody is asked.
//!
//! The "interface" side here is a raw socket speaking the wire protocol
//! directly, exactly as `dispatchd/tests/serves_clients.rs` does — a real
//! `dispatch --attach` TUI is not needed to approve or deny a request, and a
//! raw connection is far less to drive than a pseudoterminal and a screen
//! reader.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use dispatch_core::Placement;
use dispatch_os::ipc::Connection;
use dispatch_proto::{ClientMessage, Frame, ServerMessage};

/// How long to wait for the daemon or the shim to do anything before failing.
const PATIENCE: Duration = Duration::from_secs(20);

/// A temporary configuration directory holding two harnesses: one delegable,
/// one not.
struct Config {
    dir: PathBuf,
}

impl Config {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);

        // Short, because the daemon's socket lives in it: macOS's temporary
        // directory is already about fifty characters deep, and a Unix socket
        // path must fit in 104.
        let dir = std::env::temp_dir().join(format!(
            "dds-{}-{label}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let harnesses = dir.join("harnesses");
        std::fs::create_dir_all(&harnesses).expect("temp dir is writable");

        // Delegable: has a `[task]` form, so a request against it can be asked
        // about at all. The task is now a whole handoff, so the harness runs a
        // small script that pulls the Goal section out of it and runs that as
        // a script; a test still says what the subagent does as a command.
        let runner = if cfg!(windows) {
            let path = harnesses.join("run-goal.ps1");
            std::fs::write(
                &path,
                "$brief = Get-Content -Raw -LiteralPath $env:DISPATCH_TASK_FILE.Trim('\"')\n\
                 $m = [regex]::Match($brief, '(?ms)^## Goal\\s*\\r?\\n(.*?)(?=^## |\\z)')\n\
                 Invoke-Expression $m.Groups[1].Value.Trim()\n",
            )
            .expect("temp dir is writable");
            path
        } else {
            let path = harnesses.join("run-goal.sh");
            std::fs::write(
                &path,
                "goal=$(printf '%s\\n' \"$1\" | awk '/^## Goal[[:space:]]*$/{f=1;next} /^## /{f=0} f')\n\
                 eval \"$goal\"\n",
            )
            .expect("temp dir is writable");
            path
        };
        let shell = if cfg!(windows) {
            format!(
                "id = \"shell\"\ndisplay_name = \"Shell\"\ncommand = \"cmd.exe\"\n\n[task]\n\
                 args = [\"/d\", \"/v:off\", \"/c\", \"powershell.exe\", \"-NoProfile\", \"-NonInteractive\", \
                 \"-ExecutionPolicy\", \"Bypass\", \"-File\", '{}']\ninput = \"file\"\n",
                runner.display()
            )
        } else {
            format!(
                "id = \"shell\"\ndisplay_name = \"Shell\"\ncommand = \"sh\"\n\n[task]\n\
                 args = ['{}', \"{{task}}\"]\n\n[task.interactive]\nargs = ['{}', \"{{task}}\"]\n",
                runner.display(),
                runner.display()
            )
        };
        std::fs::write(harnesses.join("shell.toml"), shell).expect("temp dir is writable");

        // Not delegable: no `[task]` section at all, so the daemon refuses a
        // request against it before ever asking anybody.
        let no_task = if cfg!(windows) {
            "id = \"no-task\"\ndisplay_name = \"No Task\"\ncommand = \"cmd.exe\"\n"
        } else {
            "id = \"no-task\"\ndisplay_name = \"No Task\"\ncommand = \"sh\"\n"
        };
        std::fs::write(harnesses.join("no-task.toml"), no_task).expect("temp dir is writable");

        Self { dir }
    }

    fn with_delegation_config(label: &str, request_timeout_secs: u64) -> Self {
        let cfg = Self::new(label);
        let delegation_config = format!(
            "[delegation]\nrequest_timeout_secs = {}\n",
            request_timeout_secs
        );
        std::fs::write(cfg.dir.join("config.toml"), delegation_config)
            .expect("temp dir is writable");
        cfg
    }

    fn env(&self) -> (&'static str, String) {
        (
            dispatch_os::paths::CONFIG_DIR_ENV,
            self.dir.display().to_string(),
        )
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// How long a `dispatch delegate` call gets before a test gives up on it.
///
/// Generously more than any of these tests needs, and far less than the shim's
/// own twenty-four hour backstop, which exists for a human who may be at lunch.
const SHIM_PATIENCE: Duration = Duration::from_secs(60);

/// A running `dispatchd`, killed when the test ends however it ends.
struct Daemon(std::process::Child, PathBuf);

impl Daemon {
    fn start(config: &Config, project: &std::path::Path) -> Self {
        // A sibling of this package's own binary. Cargo only defines
        // `CARGO_BIN_EXE_<name>` for binaries of the package under test, so
        // `dispatchd` — a separate package — is found by path instead: it is
        // whatever the last build left next to `dispatch`. Run these against
        // the workspace (`cargo test --workspace`, as CI does); `cargo test -p
        // dispatch` alone will happily test a stale daemon.
        let mut path = PathBuf::from(env!("CARGO_BIN_EXE_dispatch"));
        path.set_file_name(if cfg!(windows) {
            "dispatchd.exe"
        } else {
            "dispatchd"
        });
        assert!(
            path.exists(),
            "{} is missing; build the workspace first",
            path.display()
        );

        let (key, value) = config.env();
        let child = Command::new(&path)
            .arg(project)
            .env(key, value)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the dispatchd binary can be started");

        Self(child, config.dir.clone())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();

        // Only when the test is already failing, and only here, because the
        // fixture directory is about to go. A client that connects and then
        // waits for a handshake that never comes cannot say whether the daemon
        // bound, accepted, or answered — the daemon's log can.
        if std::thread::panicking() {
            eprintln!(
                "--- dispatchd.log ({}) ---\n{}",
                self.1.display(),
                log_tail(&self.1.join("dispatchd.log"))
            );
        }
    }
}

/// The last lines of a log, or a note saying why there are none.
fn log_tail(path: &std::path::Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(60);
            lines[start..].join("\n")
        }
        Err(error) => format!("({}: {error})", path.display()),
    }
}

/// Connects to `config`'s daemon, retrying until it has bound the endpoint.
///
/// Connects to an explicit socket path rather than through
/// `Connection::connect()` (which reads `DISPATCH_CONFIG_DIR` from this
/// process's own environment): these tests only ever set that variable on the
/// child processes they spawn, so several can run in parallel without racing
/// each other over one process-wide variable.
fn connect(config: &Config) -> Connection {
    let endpoint = config.dir.join("dispatchd.sock");
    let deadline = Instant::now() + PATIENCE;

    loop {
        match Connection::connect_to(&endpoint) {
            Ok(connection) => return connection,
            Err(error) if Instant::now() >= deadline => {
                panic!("the daemon never started listening: {error}")
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Reads frames on a thread, so a message that never arrives fails the test
/// instead of blocking it forever.
fn read_in_background(mut reader: impl Read + Send + 'static) -> Receiver<ServerMessage> {
    let (sender, receiver) = channel();

    std::thread::spawn(move || {
        while let Ok(message) = Frame::read::<_, ServerMessage>(&mut reader) {
            if sender.send(message).is_err() {
                break;
            }
        }
    });

    receiver
}

/// Collects messages until `predicate` holds, or fails.
fn wait_for(
    inbox: &Receiver<ServerMessage>,
    what: &str,
    predicate: impl Fn(&[ServerMessage]) -> bool,
) -> Vec<ServerMessage> {
    let deadline = Instant::now() + PATIENCE;
    let mut seen = Vec::new();

    while Instant::now() < deadline {
        if predicate(&seen) {
            return seen;
        }

        match inbox.recv_timeout(Duration::from_millis(200)) {
            Ok(message) => seen.push(message),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if predicate(&seen) {
        return seen;
    }
    panic!("timed out waiting for {what}; saw {seen:#?}");
}

/// Connects, says hello as an interface, and subscribes.
fn attach(config: &Config) -> (Receiver<ServerMessage>, impl std::io::Write) {
    let (reader, mut writer) = connect(config).split();
    let inbox = read_in_background(reader);

    Frame::write(
        &mut writer,
        &ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate-shim test".into(),
            role: dispatch_proto::Role::Interface,
        },
    )
    .expect("writing succeeds");
    let welcome = wait_for(&inbox, "a welcome", |m| !m.is_empty());
    assert!(
        matches!(welcome.first(), Some(ServerMessage::Welcome { .. })),
        "expected a welcome, got {welcome:#?}"
    );

    Frame::write(&mut writer, &ClientMessage::Subscribe).expect("writing succeeds");

    (inbox, writer)
}

/// Opens a project, spawns a `shell` pane through it, and returns the pane.
fn spawn_parent_pane(
    ui: &Receiver<ServerMessage>,
    ui_writer: &mut impl std::io::Write,
) -> dispatch_core::PaneId {
    let announced = wait_for(ui, "the project", |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::ProjectOpened { .. }))
    });
    let project = announced
        .iter()
        .find_map(|m| match m {
            ServerMessage::ProjectOpened { project } => Some(project.id),
            _ => None,
        })
        .expect("checked by wait_for");

    Frame::write(
        ui_writer,
        &ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    )
    .expect("writing succeeds");
    let spawned = wait_for(ui, "the parent pane", |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    spawned
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("checked by wait_for")
}

/// A complete handoff whose Goal the test harness runs as a script.
fn handoff_for(goal: &str) -> String {
    format!(
        "## Goal\n{goal}\n\n## Context\nA test.\n\n## Constraints\nNone.\n\n\
         ## Done when\nIt has run.\n\n## Report back\nNothing.\n"
    )
}

/// Runs `dispatch delegate --handoff -` with `handoff` on its standard input,
/// plus `extra` arguments, and waits for it to exit.
///
/// A refusal is decided the instant the daemon receives the request, with no
/// prompt and nobody to answer it, so a blocking wait is safe here: there is
/// nothing for a test-side client to do while this runs. A denial, which does
/// need one, is driven separately in its own test rather than through this
/// helper.
fn run_delegate_shim(
    config: &Config,
    parent: dispatch_core::PaneId,
    harness: &str,
    handoff: &str,
    extra: &[&str],
) -> std::process::Output {
    let child = start_delegate_shim(config, parent, harness, handoff, extra);
    finish(child)
}

/// Starts `dispatch delegate --handoff -` without waiting for it.
fn start_delegate_shim(
    config: &Config,
    parent: dispatch_core::PaneId,
    harness: &str,
    handoff: &str,
    extra: &[&str],
) -> std::process::Child {
    use std::io::Write;

    let (key, value) = config.env();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dispatch"));
    command
        .arg("delegate")
        .arg("--handoff")
        .arg("-")
        .args(extra)
        .env("DISPATCH_PANE", parent.to_string())
        .env(key, value)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !harness.is_empty() {
        command.arg("--harness").arg(harness);
    }
    let mut child = command.spawn().expect("the dispatch binary can be started");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(handoff.as_bytes())
        .expect("the shim reads its handoff");
    child
}

/// Waits for a started shim to exit, and returns what it produced.
fn finish(mut child: std::process::Child) -> std::process::Output {
    // Bounded, because the shim's own backstop is twenty-four hours: it is built
    // to wait for a person, and a person is not coming. An unbounded wait here
    // turned a Windows transport failure into a CI job that ran for an hour
    // before anyone noticed, and would have run until the six-hour limit.
    let deadline = Instant::now() + SHIM_PATIENCE;
    loop {
        match child.try_wait().expect("the shim can be waited on") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("the shim can be waited on");
                panic!(
                    "the shim never exited within {SHIM_PATIENCE:?}; stderr was {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }

    child.wait_with_output().expect("the shim can be waited on")
}

#[test]
fn a_denied_delegate_call_exits_77_with_nothing_on_stdout() {
    let config = Config::new("denied");
    let project = config.dir.join("project");
    std::fs::create_dir_all(&project).expect("temp dir is writable");
    let _daemon = Daemon::start(&config, &project);

    let (ui, mut ui_writer) = attach(&config);
    let parent = spawn_parent_pane(&ui, &mut ui_writer);

    let child = start_delegate_shim(
        &config,
        parent,
        "",
        &handoff_for("echo should-not-run"),
        &[],
    );

    let asked = wait_for(&ui, "the pending request", |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegatePending { .. }))
    });
    let request = asked
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .expect("checked by wait_for");

    Frame::write(
        &mut ui_writer,
        &ClientMessage::DelegateDecision {
            request,
            approve: false,
            blanket: false,
        },
    )
    .expect("writing succeeds");

    let output = child.wait_with_output().expect("the shim can be waited on");

    assert_eq!(
        output.status.code(),
        Some(77),
        "a denied request exits NOPERM; stderr was {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "a denied request never runs anything, so stdout must be empty, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_refused_delegate_call_exits_78_with_nothing_on_stdout() {
    // Refused before anyone is asked: `no-task` has no `[task]` form, so this
    // needs no interface client at all, only the parent pane to ask from.
    let config = Config::new("refused");
    let project = config.dir.join("project");
    std::fs::create_dir_all(&project).expect("temp dir is writable");
    let _daemon = Daemon::start(&config, &project);

    let (ui, mut ui_writer) = attach(&config);
    let parent = spawn_parent_pane(&ui, &mut ui_writer);

    let output = run_delegate_shim(
        &config,
        parent,
        "no-task",
        &handoff_for("echo should-not-run"),
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(78),
        "a harness with no [task] form is refused with CONFIG; stderr was {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "a refused request never runs anything, so stdout must be empty, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn an_unanswered_delegate_call_exits_75_when_the_deadline_passes() {
    // Expired when nobody answers: the daemon is asked but no interface client
    // approves or denies before the deadline. The shim asks with no interface
    // client listening, so the request times out.
    let config = Config::with_delegation_config("expired", 0);
    let project = config.dir.join("project");
    std::fs::create_dir_all(&project).expect("temp dir is writable");
    let _daemon = Daemon::start(&config, &project);

    let (ui, mut ui_writer) = attach(&config);
    let parent = spawn_parent_pane(&ui, &mut ui_writer);

    // Run the shim without answering the pending request. The daemon will ask
    // on the interface connection we hold, but we do not answer it.
    let output = run_delegate_shim(
        &config,
        parent,
        "",
        &handoff_for("echo should-not-run"),
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(75),
        "an unanswered request exits TEMPFAIL; stderr was {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "an unanswered request never runs anything, so stdout must be empty, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_stale_dispatch_pane_exits_75_rather_than_blaming_the_configuration() {
    // `DISPATCH_PANE` goes stale the moment the daemon restarts: the agent is
    // typing in a new pane with a new id, and the value it was given names
    // nothing. That is "try again", not "your configuration is wrong", which is
    // what 78 tells an agent — it would go looking for a problem that is not
    // there. Same argument as a timed-out request, same code.
    // A short label deliberately: the daemon's endpoint lives inside this
    // directory, and a Unix socket address is limited to about a hundred bytes.
    let config = Config::new("stale");
    let project = config.dir.join("project");
    std::fs::create_dir_all(&project).expect("temp dir is writable");
    let _daemon = Daemon::start(&config, &project);

    // Wait for the endpoint before running the shim. Without this the shim can
    // reach the socket first and exit 69 for a daemon that is merely still
    // starting — which is what CI saw while this machine won the race.
    drop(connect(&config));

    // A pane id no daemon ever owned stands in for one whose daemon was
    // restarted under it.
    let output = run_delegate_shim(
        &config,
        dispatch_core::PaneId::new(),
        "shell",
        &handoff_for("echo should-not-run"),
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(75),
        "a pane the daemon does not know exits TEMPFAIL; stderr was {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "nothing ran, so stdout must be empty, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Runs `dispatch` with `args` and no daemon involved, with `stdin`.
fn run_plain(args: &[&str], stdin: &[u8], pane: Option<&str>) -> std::process::Output {
    use std::io::Write;

    let mut command = Command::new(env!("CARGO_BIN_EXE_dispatch"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pane) = pane {
        command.env("DISPATCH_PANE", pane);
    }
    let mut child = command.spawn().expect("starts");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(stdin)
        .expect("writes");
    child.wait_with_output().expect("exits")
}

#[test]
fn the_template_is_printed_without_a_daemon() {
    let output = run_plain(&["delegate", "--template"], b"", None);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("## Goal"), "{text}");
}

#[test]
fn a_bare_task_is_refused_with_the_template() {
    let output = run_plain(
        &["delegate", "do the thing"],
        b"",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(64));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        errors.contains("--handoff") && errors.contains("## Report back"),
        "{errors}"
    );
}

#[test]
fn an_incomplete_handoff_is_refused_naming_what_is_missing() {
    let output = run_plain(
        &["delegate", "--handoff", "-"],
        b"## Goal\nsomething\n",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(64));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("Context is missing"), "{errors}");
}

#[test]
fn an_unreadable_handoff_file_is_66() {
    let output = run_plain(
        &["delegate", "--handoff", "/nonexistent/dispatch/handoff.md"],
        b"",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(66));
}

#[test]
fn an_empty_report_is_64() {
    let output = run_plain(
        &["report", "-"],
        b"  \n",
        Some(&dispatch_core::PaneId::new().to_string()),
    );
    assert_eq!(output.status.code(), Some(64));
}

#[test]
fn the_subagents_report_reaches_the_callers_stdout() {
    let config = Config::new("report-e2e");
    let project = std::env::temp_dir();
    let _daemon = Daemon::start(&config, &project);
    let (ui, mut ui_writer) = attach(&config);
    let parent = spawn_parent_pane(&ui, &mut ui_writer);

    // The report holds a byte that is not UTF-8: it is still delivered. On
    // Windows it comes from a file, since PowerShell pipes a byte array to a
    // program as one line of decimal digits per byte.
    let goal = if cfg!(windows) {
        let report = config.dir.join("report.bin");
        std::fs::write(&report, b"found\xff").expect("temp dir is writable");
        format!("dispatch report '{}'", report.display())
    } else {
        "printf 'found\\377' | dispatch report -".to_string()
    };
    let child = start_delegate_shim(&config, parent, "", &handoff_for(&goal), &[]);

    let asked = wait_for(&ui, "the prompt", |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegatePending { .. }))
    });
    let request = asked
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .expect("checked");
    Frame::write(
        &mut ui_writer,
        &ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    )
    .expect("writing succeeds");

    let output = finish(child);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("found"),
        "the report, not the tail: {stdout:?}"
    );
}
