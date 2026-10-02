//! Tests for the daemon loop.
//!
//! Each drives the daemon directly rather than through a socket: the transport
//! is tested in `dispatch-os`, and driving the loop keeps these about what the
//! daemon decides rather than how bytes travel.

use super::*;

use std::time::Instant;

use dispatch_core::Placement;

/// A harness registry holding a plain shell, so panes run something real.
///
/// Carries a `[task]` form so delegation tests have a harness to delegate to;
/// without one, every delegation request is refused before it is even asked
/// about. Its task is a script. On Windows that script is read by PowerShell
/// from the task's file, since `cmd.exe /c {task}` is the very shape the
/// daemon refuses there; `powershell -Command -` runs what arrives on standard
/// input, where `echo` prints and `sleep` sleeps as they do under `sh`.
///
/// Also registers `no-task-args`: a harness with a `[task]` section but an
/// empty `args`, which `HarnessDef::task_launch` treats as no form at all — a
/// fixture for the difference between `task.is_some()` and
/// `task_launch(..).is_some()`.
fn harnesses(dir: &std::path::Path) -> HarnessRegistry {
    std::fs::create_dir_all(dir).expect("temp dir is writable");
    // The subagent is handed a brief: a preamble, then the handoff. These
    // runners pull the Goal section out of it and run that as a script, so
    // a test still says what the subagent does as a shell command.
    let runner = if cfg!(windows) {
        let path = dir.join("run-goal.ps1");
        std::fs::write(
            &path,
            "$brief = Get-Content -Raw -LiteralPath $env:DISPATCH_TASK_FILE.Trim('\"')\n\
             $m = [regex]::Match($brief, '(?ms)^## Goal\\s*\\r?\\n(.*?)(?=^## |\\z)')\n\
             Invoke-Expression $m.Groups[1].Value.Trim()\n",
        )
        .expect("temp dir is writable");
        path
    } else {
        let path = dir.join("run-goal.sh");
        std::fs::write(
            &path,
            "goal=$(printf '%s\\n' \"$1\" | awk '/^## Goal[[:space:]]*$/{f=1;next} /^## /{f=0} f')\n\
             eval \"$goal\"\n",
        )
        .expect("temp dir is writable");
        path
    };
    let body = if cfg!(windows) {
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
    std::fs::write(dir.join("shell.toml"), body).expect("temp dir is writable");

    let no_task_args = if cfg!(windows) {
        "id = \"no-task-args\"\ndisplay_name = \"No task args\"\ncommand = \"cmd.exe\"\n\n[task]\nargs = []\n"
    } else {
        "id = \"no-task-args\"\ndisplay_name = \"No task args\"\ncommand = \"sh\"\n\n[task]\nargs = []\n"
    };
    std::fs::write(dir.join("no-task-args.toml"), no_task_args).expect("temp dir is writable");

    // Puts its terminal in raw mode so input is buffered rather than
    // processed, says so, and never reads: an agent busy elsewhere when a
    // paste arrives. On Windows it only never reads for itself; see
    // `a_stalled_pane_does_not_stall_the_daemon` for why that is not enough.
    let stall = if cfg!(windows) {
        "id = \"stall\"\ndisplay_name = \"Stall\"\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"echo READY & ping -n 30 127.0.0.1 >nul\"]\n"
    } else {
        "id = \"stall\"\ndisplay_name = \"Stall\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; echo READY; sleep 30\"]\n"
    };
    std::fs::write(dir.join("stall.toml"), stall).expect("temp dir is writable");

    // Prints as fast as it can, forever.
    let flood = if cfg!(windows) {
        "id = \"flood\"\ndisplay_name = \"Flood\"\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"for /l %i in (0,0,1) do @echo flood\"]\n"
    } else {
        "id = \"flood\"\ndisplay_name = \"Flood\"\ncommand = \"yes\"\nargs = [\"flood\"]\n"
    };
    std::fs::write(dir.join("flood.toml"), flood).expect("temp dir is writable");

    // Prints the numbers from one to COUNTED, one to a line, and then waits:
    // output in which every byte has one place, so what a client is replayed
    // can be held against exactly what the pane printed. Through `cat`, so it
    // arrives in large writes: `seq` writing to a terminal writes a line at a
    // time, and a pane is handed a bounded number of reads per tick.
    let count = if cfg!(windows) {
        format!(
            "id = \"count\"\ndisplay_name = \"Count\"\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"(for /l %i in (1,1,{COUNTED}) do @echo %i) & ping -n 30 127.0.0.1 >nul\"]\n"
        )
    } else {
        format!(
            "id = \"count\"\ndisplay_name = \"Count\"\ncommand = \"sh\"\nargs = [\"-c\", \"seq 1 {COUNTED} | cat; sleep 30\"]\n"
        )
    };
    std::fs::write(dir.join("count.toml"), count).expect("temp dir is writable");

    // A pane with a grandchild, so shutdown can be seen to end the whole tree.
    let tree = if cfg!(windows) {
        "id = \"tree\"\ndisplay_name = \"Tree\"\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"ping -n 30 127.0.0.1 >nul\"]\n"
    } else {
        "id = \"tree\"\ndisplay_name = \"Tree\"\ncommand = \"sh\"\nargs = [\"-c\", \"sleep 30 & sleep 30\"]\n"
    };
    std::fs::write(dir.join("tree.toml"), tree).expect("temp dir is writable");

    // Asks its terminal who it is, shows the answer with the escape as `E`,
    // then shows whatever else arrives within half a second: a second
    // answer would be there.
    if !cfg!(windows) {
        std::fs::write(
            dir.join("asker.toml"),
            "id = \"asker\"\ndisplay_name = \"Asker\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; printf '\\\\033[c'; head -c 9 | tr '\\\\033' E; stty min 0 time 5; extra=$(head -c 64 | wc -c); printf '\\\\r\\\\nEXTRA:%s\\\\r\\\\n' $extra; sleep 30\"]\n",
        )
        .expect("temp dir is writable");

        // Asks as `asker` does, but only after a second: a test can have every
        // window gone before the question is printed.
        std::fs::write(
            dir.join("late-asker.toml"),
            "id = \"late-asker\"\ndisplay_name = \"Late asker\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; sleep 1; printf '\\\\033[c'; head -c 9 | tr '\\\\033' E; stty min 0 time 5; extra=$(head -c 64 | wc -c); printf '\\\\r\\\\nEXTRA:%s\\\\r\\\\n' $extra; sleep 30\"]\n",
        )
        .expect("temp dir is writable");

        // Waits for one byte of input before asking for its size, so a test
        // can resize the pane first.
        std::fs::write(
            dir.join("sizer.toml"),
            "id = \"sizer\"\ndisplay_name = \"Sizer\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; head -c 1 >/dev/null; printf '\\\\033[18t'; head -c 11 | tr '\\\\033' E; sleep 30\"]\n",
        )
        .expect("temp dir is writable");

        // Writes 100,000 queries and never reads its input.
        std::fs::write(
            dir.join("flooder.toml"),
            "id = \"flooder\"\ndisplay_name = \"Flooder\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; i=0; while [ $i -lt 100000 ]; do printf '\\\\033[c'; i=$((i+1)); done; sleep 30\"]\n",
        )
        .expect("temp dir is writable");

        // Waits for one byte of input, turns on in-band size reports, then
        // shows what it is sent over the next three seconds, so the report a
        // resize causes can be seen arriving. The wait is so a test sees the
        // mode go on: output printed at once can land in the batch that
        // carries `PaneSpawned`, which `spawn_harness` discards. `tr` buffers
        // its output until its input ends, and `head` reading under `VTIME`
        // is what ends it.
        std::fs::write(
            dir.join("reporter.toml"),
            "id = \"reporter\"\ndisplay_name = \"Reporter\"\ncommand = \"sh\"\nargs = [\"-c\", \"stty raw -echo; head -c 1 >/dev/null; printf '\\\\033[?2048h'; stty min 0 time 30; head -c 64 | tr '\\\\033' E; sleep 30\"]\n",
        )
        .expect("temp dir is writable");
    }

    HarnessRegistry::load_from_dir(dir).expect("loading succeeds")
}

/// How far the `count` harness counts.
///
/// About 2 MB of output: well past what a client that never reads can hold
/// in its socket and its outbox, so it is hung up, and far past the history
/// a late client is replayed.
const COUNTED: u32 = 300_000;

/// A temporary directory that cleans itself up.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);

        let path = std::env::temp_dir().join(format!(
            "dispatch-daemon-{}-{label}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("temp dir is writable");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A daemon with one project registered, plus that project's id.
fn daemon(label: &str) -> (Daemon, ProjectId, TempDir) {
    let dir = TempDir::new(label);
    let registry = harnesses(&dir.0.join("harnesses"));

    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");
    let project = daemon.open_project(root);

    (daemon, project, dir)
}

fn hello() -> ClientMessage {
    ClientMessage::Hello {
        version: dispatch_proto::VERSION,
        client: "test".into(),
        role: dispatch_proto::Role::Interface,
    }
}

/// Everything a pane printed, as text, across the messages seen.
fn output_of(messages: &[ServerMessage], pane: PaneId) -> String {
    let bytes: Vec<u8> = messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::PaneOutput { pane: p, bytes } if *p == pane => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect();

    String::from_utf8_lossy(&bytes).into_owned()
}

/// Drains whatever a client has been sent.
fn drain(inbox: &Inbox) -> Vec<ServerMessage> {
    let mut messages = Vec::new();
    while let Ok(message) = inbox.try_recv() {
        messages.push(message);
    }
    messages
}

/// How long [`wait_for`] ticks the daemon before giving up.
///
/// A deadline, not a delay: a passing test returns the moment its predicate
/// holds, so this costs only a test that is failing anyway. Thirty seconds
/// because a delegation test on Windows starts two cold PowerShell one-shots
/// back to back, and a loaded runner once took longer than ten over them.
const WAIT_FOR_DEADLINE: Duration = Duration::from_secs(30);

/// Ticks the daemon until `predicate` holds, or gives up.
fn wait_for(
    daemon: &mut Daemon,
    inbox: &Inbox,
    predicate: impl Fn(&[ServerMessage]) -> bool,
) -> Vec<ServerMessage> {
    let mut seen = Vec::new();
    let deadline = Instant::now() + WAIT_FOR_DEADLINE;

    loop {
        daemon.tick();
        seen.extend(drain(inbox));

        if predicate(&seen) {
            return seen;
        }
        if Instant::now() >= deadline {
            panic!("timed out; saw {seen:#?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_matching_version_is_welcomed() {
    let (mut daemon, _, _dir) = daemon("welcome");
    let inbox = daemon.attach_for_test(1);

    daemon.request_for_test(1, hello());

    let messages = drain(&inbox);
    assert!(
        matches!(messages.first(), Some(ServerMessage::Welcome { .. })),
        "expected a welcome, got {messages:?}"
    );
}

#[test]
fn an_incompatible_major_version_is_refused_and_the_client_dropped() {
    // Proceeding would let the peer misread everything sent after the
    // handshake, so the connection ends here.
    let (mut daemon, _, _dir) = daemon("refuse");
    let inbox = daemon.attach_for_test(1);

    daemon.request_for_test(
        1,
        ClientMessage::Hello {
            version: dispatch_proto::Version {
                major: 99,
                minor: 0,
            },
            client: "from the future".into(),
            role: dispatch_proto::Role::Interface,
        },
    );

    let messages = drain(&inbox);
    assert!(
        matches!(
            messages.first(),
            Some(ServerMessage::Error {
                error: ProtocolError::IncompatibleVersion { .. }
            })
        ),
        "expected a version error, got {messages:?}"
    );

    // Nothing further reaches a refused client.
    daemon.request_for_test(1, ClientMessage::Ping { token: 1 });
    assert!(drain(&inbox).is_empty(), "a refused client must be dropped");
}

#[test]
fn a_ping_is_answered_with_its_token() {
    let (mut daemon, _, _dir) = daemon("ping");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    daemon.request_for_test(1, ClientMessage::Ping { token: 7 });

    assert_eq!(drain(&inbox), vec![ServerMessage::Pong { token: 7 }]);
}

#[test]
fn spawning_a_pane_starts_a_process_and_tells_the_client() {
    let (mut daemon, project, _dir) = daemon("spawn");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    assert_eq!(daemon.pane_count(), 1);
    assert!(
        seen.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    );
}

#[test]
fn pane_output_reaches_the_client() {
    let (mut daemon, project, _dir) = daemon("output");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo daemon-works\r".to_vec(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |messages| {
        messages.iter().any(|m| match m {
            ServerMessage::PaneOutput { bytes, .. } => {
                String::from_utf8_lossy(bytes).contains("daemon-works")
            }
            _ => false,
        })
    });

    assert!(!seen.is_empty());
}

/// Waits, without ticking the daemon, until it has read some of `pane`'s
/// output and not yet sent it: what a resize then has to deal with.
///
/// A bounded poll rather than a fixed sleep, so a slow machine waits longer
/// instead of failing. `request_for_test`, unlike `Daemon::tick`, never
/// drains panes on its own, so what is waiting stays waiting.
fn until_output_is_waiting(daemon: &Daemon, pane: PaneId) {
    let deadline = Instant::now() + WAIT_FOR_DEADLINE;
    while daemon.pane_waiting_for_test(pane) == 0 {
        assert!(
            Instant::now() < deadline,
            "the pane's output was never read"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `fit` must send what the daemon has already read for a pane before it
/// broadcasts `PaneResized`, not only what `pump_panes` happens to have
/// drained by the time a resize lands: a tick drains a bounded amount.
#[test]
fn fit_sends_what_it_already_read_before_it_resizes() {
    let (mut daemon, project, _dir) = daemon("fit-drains-first");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_pane_for_test(&mut daemon, &ui, project);
    let _ = drain(&ui);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo before-resize-marker\r".to_vec(),
        },
    );
    until_output_is_waiting(&daemon, pane);

    daemon.request_for_test(
        1,
        ClientMessage::ResizePane {
            pane,
            size: (100, 30),
        },
    );

    // Nothing ticked the daemon since the output was read, so any output
    // here was sent by the resize itself.
    let seen = drain(&ui);
    let output_at = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneOutput { pane: p, .. } if *p == pane));
    let resized_at = seen.iter().position(|m| {
        matches!(
            m,
            ServerMessage::PaneResized { pane: p, size: (100, 30) } if *p == pane
        )
    });

    assert!(
        output_at.is_some(),
        "fit did not send what the daemon had already read: {seen:#?}"
    );
    assert!(
        resized_at.is_some(),
        "the resize itself was never sent: {seen:#?}"
    );
    assert!(
        output_at < resized_at,
        "output the daemon had already read must reach every window before \
         PaneResized, not after: {seen:#?}"
    );
}

#[test]
fn a_window_leaving_while_its_pane_is_resized_leaves_the_right_size() {
    // Sending a pane's waiting output ahead of a resize can hang up a window
    // that has fallen behind. If that window was the one deciding the size,
    // its departure hands the pane to the next window mid-resize, and the
    // resize must not then go ahead with the size of the window that left.
    let (mut daemon, project, _dir) = daemon("fit-decider-leaves");
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    // Window 2 is never read from here on: it is the one that falls behind.
    let _second = attach_window(&mut daemon, 2);
    // Window 1's size takes hold (window 2 has asked for none), and the
    // `PaneResized` saying so is left queued, unread, for window 2: traffic
    // it is behind on.
    ask_size(&mut daemon, 1, pane, 90, 28);
    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(90, 28)));

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo waiting\r".to_vec(),
        },
    );
    until_output_is_waiting(&daemon, pane);
    daemon.request_for_test(2, ClientMessage::Active);
    let _ = drain(&first);

    // Past this point, a client with anything live queued and unread is
    // `Refused::Behind` on its next send: window 2 is, window 1 is not.
    daemon.set_budgets(Budgets {
        outbox_bytes: 0,
        ..Budgets::default()
    });

    // Window 2, the one in use, asks for 60x20. Sending the waiting output
    // first hangs it up, and the pane goes back to window 1's size.
    ask_size(&mut daemon, 2, pane, 60, 20);

    assert_eq!(
        daemon.pane_size_for_test(pane),
        Some(Size::new(90, 28)),
        "window 2 was hung up before its size took hold, so window 1's stands"
    );
}

#[test]
fn every_subscribed_client_sees_the_same_panes() {
    // This is what lets a MacBook and a desktop show one fleet, so output goes
    // to all of them rather than to whoever asked.
    let (mut daemon, project, _dir) = daemon("broadcast");

    let first = daemon.attach_for_test(1);
    let second = daemon.attach_for_test(2);
    for id in [1, 2] {
        daemon.request_for_test(id, hello());
        daemon.request_for_test(id, ClientMessage::Subscribe);
    }
    let _ = drain(&first);
    let _ = drain(&second);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    daemon.tick();

    let saw_spawn = |inbox: &Inbox| {
        drain(inbox)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    };

    assert!(saw_spawn(&first), "the client that asked should be told");
    assert!(saw_spawn(&second), "so should every other client");
}

#[test]
fn a_client_that_has_not_subscribed_is_left_quiet() {
    // A one-shot command should not be sent a session's worth of output.
    let (mut daemon, project, _dir) = daemon("quiet");

    let subscriber = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);

    let silent = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    let _ = drain(&silent);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    daemon.tick();

    assert!(
        !drain(&subscriber).is_empty(),
        "the subscriber hears about it"
    );
    assert!(drain(&silent).is_empty(), "the other client stays quiet");
}

#[test]
fn a_client_attaching_later_is_told_what_already_exists() {
    // Reattaching from another machine must show the running panes rather
    // than an empty screen until something changes.
    let (mut daemon, project, _dir) = daemon("catch-up");

    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &first, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let messages = drain(&late);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. })),
        "a late client should be told about existing panes, got {messages:?}"
    );
}

#[test]
fn panes_outlive_the_client_that_started_them() {
    // The whole point of the split: close the laptop, the work keeps running.
    let (mut daemon, project, _dir) = daemon("outlive");

    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    daemon.detach_for_test(1);
    daemon.tick();

    assert_eq!(
        daemon.pane_count(),
        1,
        "the pane must survive its client detaching"
    );
}

#[test]
fn closing_a_pane_removes_it_and_tells_everyone() {
    let (mut daemon, project, _dir) = daemon("close");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");

    daemon.request_for_test(1, ClientMessage::ClosePane { pane });

    assert_eq!(daemon.pane_count(), 0);
    assert!(
        drain(&inbox)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { .. })),
        "closing should be broadcast"
    );
}

#[test]
fn acting_on_an_unknown_pane_is_reported_rather_than_ignored() {
    let (mut daemon, _, _dir) = daemon("unknown-pane");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    let ghost = PaneId::new();
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: ghost,
            bytes: b"x".to_vec(),
        },
    );

    assert!(
        drain(&inbox).iter().any(|m| matches!(
            m,
            ServerMessage::Error {
                error: ProtocolError::NoSuchPane(_)
            }
        )),
        "a write to a pane that is gone should say so"
    );
}

#[test]
fn spawning_into_an_unknown_project_is_reported() {
    let (mut daemon, _, _dir) = daemon("unknown-project");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project: ProjectId::new(),
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    assert!(drain(&inbox).iter().any(|m| matches!(
        m,
        ServerMessage::Error {
            error: ProtocolError::NoSuchProject(_)
        }
    )));
}

#[test]
fn spawning_an_unknown_harness_is_reported_with_its_name() {
    let (mut daemon, project, _dir) = daemon("unknown-harness");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "nonexistent".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let messages = drain(&inbox);
    let explained = messages.iter().any(|m| match m {
        ServerMessage::Error {
            error: ProtocolError::Other(text),
        } => text.contains("nonexistent"),
        _ => false,
    });

    assert!(
        explained,
        "the error should name the harness, got {messages:?}"
    );
}

#[test]
fn an_exited_pane_is_reported_and_kept() {
    // Its final output is still worth reading, so it stays until closed.
    let (mut daemon, project, _dir) = daemon("exit");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"exit 0\r".to_vec(),
        },
    );

    wait_for(&mut daemon, &inbox, |messages| {
        messages.iter().any(|m| {
            matches!(
                m,
                ServerMessage::PaneChanged {
                    update: PaneUpdate::Status {
                        status: PaneStatus::Exited(_)
                    },
                    ..
                }
            )
        })
    });

    assert_eq!(daemon.pane_count(), 1, "an exited pane stays until closed");
}

#[test]
fn a_shutdown_handle_reports_the_request() {
    let (daemon, _, _dir) = daemon("handle");
    let handle = daemon.shutdown_handle();

    assert!(!handle.is_requested(), "a fresh daemon is not stopping");
    handle.request();
    assert!(handle.is_requested());
}

#[test]
fn a_requested_shutdown_stops_the_loop_and_kills_the_panes() {
    let (mut daemon, project, dir) = daemon("shutdown");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    assert_eq!(daemon.pane_count(), 1);

    // The loop runs on another thread so a missing shutdown check fails the
    // test rather than hanging it.
    let shutdown = daemon.shutdown_handle();
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        daemon.run();
        let _ = done.send(daemon.pane_count());
        drop(dir);
    });

    shutdown.request();

    let remaining = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("run returns once a shutdown is requested");
    assert_eq!(
        remaining, 0,
        "panes are the daemon's children and must not outlive it"
    );

    worker.join().expect("the loop thread does not panic");
}

#[test]
fn reopening_a_root_keeps_one_project() {
    // Two sidebar entries for one checkout would be a bug the user has to
    // untangle by hand.
    let (mut daemon, project, dir) = daemon("reopen");
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");

    assert_eq!(daemon.open_project(root), project);
    assert_eq!(daemon.projects().len(), 1);
}

#[test]
fn a_subscriber_is_told_the_projects_before_the_panes() {
    // A pane names the project it belongs to, so a client that heard about the
    // pane first would have nowhere to put it.
    let (mut daemon, project, _dir) = daemon("projects-first");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    // A second client attaching now sees the whole picture.
    let later = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let seen = drain(&later);
    let projects = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::ProjectOpened { .. }))
        .expect("the project is announced");
    let panes = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
        .expect("the pane is announced");
    assert!(projects < panes, "projects come first, saw {seen:#?}");

    let Some(ServerMessage::ProjectOpened { project: opened }) = seen.get(projects) else {
        unreachable!("checked above");
    };
    assert_eq!(opened.id, project);
}

#[test]
fn opening_a_project_tells_every_subscriber() {
    let (mut daemon, _, dir) = daemon("open");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    let nested = dir.0.join("nested");
    std::fs::create_dir_all(&nested).expect("temp dir is writable");

    daemon.request_for_test(
        1,
        ClientMessage::OpenProject {
            root: nested.clone(),
        },
    );

    // Past the asker's own `ProjectResolved`, which comes first.
    let seen = drain(&inbox);
    let Some(project) = seen.iter().find_map(|m| match m {
        ServerMessage::ProjectOpened { project } => Some(project),
        _ => None,
    }) else {
        panic!("expected a project, got {seen:#?}");
    };
    assert_eq!(project.name, "nested");
    assert_eq!(
        project.root,
        dispatch_os::paths::resolve(&nested).expect("the nested dir resolves"),
        "the daemon resolves the path it was given"
    );
    assert_eq!(daemon.projects().len(), 2);
}

#[test]
fn opening_a_path_that_is_not_a_directory_is_reported() {
    let (mut daemon, _, dir) = daemon("open-bad");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    let file = dir.0.join("not-a-directory");
    std::fs::write(&file, b"contents").expect("temp dir is writable");

    daemon.request_for_test(1, ClientMessage::OpenProject { root: file.clone() });
    assert!(
        matches!(
            drain(&inbox).first(),
            Some(ServerMessage::ProjectRefused { root, .. }) if *root == file
        ),
        "a file is not a project, and the refusal names it as it was sent"
    );

    let missing = dir.0.join("missing");
    daemon.request_for_test(
        1,
        ClientMessage::OpenProject {
            root: missing.clone(),
        },
    );
    assert!(
        matches!(
            drain(&inbox).first(),
            Some(ServerMessage::ProjectRefused { root, .. }) if *root == missing
        ),
        "a path that does not exist is not a project"
    );

    assert_eq!(daemon.projects().len(), 1, "neither was registered");
}

#[test]
fn a_root_under_home_is_opened_from_the_daemons_own_home() {
    // The daemon is on the machine the directory is on, so its home is the
    // one `~` means. `~` itself always exists, so this needs no scratch
    // directory under the real home.
    let (mut daemon, _, _dir) = daemon("open-home");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::OpenProject {
            root: PathBuf::from("~"),
        },
    );

    // Through `expand_home` rather than `directories`, which this crate does
    // not depend on: what is under test is that the daemon expands at all.
    let home = dispatch_os::paths::expand_home(Path::new("~"));
    let expected = dispatch_os::paths::resolve(&home).expect("home resolves");

    assert!(
        drain(&inbox).iter().any(|message| matches!(
            message,
            ServerMessage::ProjectOpened { project } if project.root == expected
        )),
        "`~` should open the daemon's home"
    );
}

#[test]
fn the_asker_alone_is_told_what_its_root_resolved_to() {
    // The asker keeps `~`, but the row every client gets carries the home it
    // resolved to. Only the asker has a record to rewrite; the others were
    // never told `~`, and it would mean nothing to them.
    let (mut daemon, _, _dir) = daemon("resolved");
    let asker = daemon.attach_for_test(1);
    let other = daemon.attach_for_test(2);
    for client in [1, 2] {
        daemon.request_for_test(client, hello());
        daemon.request_for_test(client, ClientMessage::Subscribe);
    }
    let _ = drain(&asker);
    let _ = drain(&other);

    daemon.request_for_test(
        1,
        ClientMessage::OpenProject {
            root: PathBuf::from("~"),
        },
    );

    let home = dispatch_os::paths::expand_home(Path::new("~"));
    let expected = dispatch_os::paths::resolve(&home).expect("home resolves");

    let heard = drain(&asker);
    assert!(
        matches!(
            heard.as_slice(),
            [
                ServerMessage::ProjectResolved { root, resolved },
                ServerMessage::ProjectOpened { project },
                ServerMessage::Tabs { .. },
            ] if root == Path::new("~") && *resolved == expected && project.root == expected
        ),
        "the asker hears how its root resolved, before the row arrives: {heard:#?}"
    );

    let overheard = drain(&other);
    assert!(
        !overheard
            .iter()
            .any(|m| matches!(m, ServerMessage::ProjectResolved { .. })),
        "nobody else is told: {overheard:#?}"
    );
    assert!(
        overheard
            .iter()
            .any(|m| matches!(m, ServerMessage::ProjectOpened { .. })),
        "though everyone still gets the row"
    );
}

#[test]
fn a_client_attaching_later_is_replayed_what_a_pane_printed() {
    // Reattaching should show the work, not a blank rectangle.
    let (mut daemon, project, _dir) = daemon("replay");
    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &first, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo remembered-42\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &first, |messages| {
        output_of(messages, pane).contains("remembered-42")
    });

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let messages = drain(&late);
    assert!(
        output_of(&messages, pane).contains("remembered-42"),
        "a late client should be replayed the pane's output, got {messages:#?}"
    );
}

#[test]
fn a_pane_remembers_only_its_most_recent_output() {
    // An agent can print for hours; the daemon cannot keep all of it.
    let (mut daemon, project, _dir) = daemon("history-cap");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    let pane = daemon
        .panes
        .values_mut()
        .next()
        .expect("the pane is registered");

    // Fed directly: driving a real shell into producing a quarter of a megabyte
    // would make this a slow test of the shell rather than of the limit.
    pane.remember(&vec![b'a'; crate::pane::HISTORY_BYTES]);
    pane.remember(b"the newest bytes");

    let history = &daemon
        .panes
        .values()
        .next()
        .expect("the pane is registered")
        .history;
    assert_eq!(history.len(), crate::pane::HISTORY_BYTES);
    assert!(
        history.ends_with(b"the newest bytes"),
        "the newest output is what a client needs"
    );
}

#[test]
fn a_client_attaching_after_a_pane_exited_is_told_it_exited() {
    // The change happened before this client was listening, so a subscribe has
    // to carry it or the pane looks alive forever.
    let (mut daemon, project, _dir) = daemon("late-exit");
    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &first, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"exit 3\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &first, |messages| {
        messages.iter().any(|m| {
            matches!(
                m,
                ServerMessage::PaneChanged {
                    update: PaneUpdate::Status {
                        status: PaneStatus::Exited(_)
                    },
                    ..
                }
            )
        })
    });

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let messages = drain(&late);
    assert!(
        messages.iter().any(|m| matches!(
            m,
            ServerMessage::PaneChanged {
                update: PaneUpdate::Status {
                    status: PaneStatus::Exited(_)
                },
                ..
            }
        )),
        "a late client should be told the pane exited, got {messages:#?}"
    );
}

/// A daemon with one project and non-default limits.
fn daemon_with_limits(label: &str, limits: DelegationLimits) -> (Daemon, ProjectId, TempDir) {
    let dir = TempDir::new(label);
    let registry = harnesses(&dir.0.join("harnesses"));

    let mut daemon = Daemon::with_limits(registry, "test-device", limits);
    daemon.set_task_dir(dir.0.join("tasks"));
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");
    let project = daemon.open_project(root);

    (daemon, project, dir)
}

/// Spawns a pane the ordinary way and returns its id.
fn spawn_pane_for_test(daemon: &mut Daemon, inbox: &Inbox, project: ProjectId) -> PaneId {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(daemon, inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    seen.iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned")
}

/// Whether a spawn announcement is for a delegated pane.
fn m_is_child(message: &ServerMessage) -> bool {
    matches!(
        message,
        ServerMessage::PaneSpawned {
            parent: Some(_),
            ..
        }
    )
}

/// A complete handoff whose Goal is `goal`, which the test harness runs as a
/// script.
fn handoff_for(goal: &str) -> dispatch_core::Handoff {
    dispatch_core::Handoff::parse(&format!(
        "## Goal\n{goal}\n\n## Context\nA test.\n\n## Constraints\nNone.\n\n\
         ## Done when\nIt has run.\n\n## Report back\nNothing.\n"
    ))
    .expect("a complete handoff")
}

/// Attaches a delegate caller and asks for a subagent.
fn ask(daemon: &mut Daemon, parent: PaneId, task: &str) -> Inbox {
    ask_as(daemon, 9, parent, task)
}

/// Attaches a delegate caller under a specific client id and asks for a
/// subagent. Needed over `ask` when a test drives two delegate callers at
/// once, since `ask` always reuses id 9.
fn ask_as(daemon: &mut Daemon, id: u64, parent: PaneId, task: &str) -> Inbox {
    let caller = daemon.attach_for_test(id);
    daemon.request_for_test(
        id,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        id,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: task.into(),
            size: (80, 24),
            handoff: Some(handoff_for(task)),
            interactive: false,
        },
    );
    caller
}

/// The first pending request an interface client was told about.
fn pending(messages: &[ServerMessage]) -> Option<dispatch_core::RequestId> {
    messages.iter().find_map(|m| match m {
        ServerMessage::DelegatePending { request, .. } => Some(*request),
        _ => None,
    })
}

#[test]
fn a_delegation_request_is_put_to_the_user() {
    let (mut daemon, project, _dir) = daemon("delegate-ask");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let _caller = ask(&mut daemon, parent, "echo delegated");

    let seen = drain(&ui);
    let request = pending(&seen).expect("the interface is asked");
    assert!(
        matches!(
            seen.iter().find(|m| matches!(m, ServerMessage::DelegatePending { .. })),
            Some(ServerMessage::DelegatePending { handoff: Some(handoff), .. })
                if handoff.goal == "echo delegated"
        ),
        "the whole task travels, got {seen:#?}"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing runs before an answer");
    let _ = request;
}

#[test]
fn a_request_without_a_handoff_is_refused_without_asking() {
    let (mut daemon, project, _dir) = daemon("delegate-bare");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "old delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: "echo bare".into(),
            size: (80, 24),
            handoff: None,
            interactive: false,
        },
    );

    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("--handoff"), "{reason}");
    assert!(pending(&drain(&ui)).is_none(), "nobody is asked");
}

#[test]
fn a_handoff_with_problems_is_refused_even_when_the_caller_skipped_the_check() {
    let (mut daemon, project, _dir) = daemon("delegate-broken");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let mut broken = handoff_for("echo x");
    broken.text = "## Goal\necho x\n".into();

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "careless".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(broken),
            interactive: false,
        },
    );

    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("Context is missing"), "{reason}");
}

#[test]
fn the_subagent_is_handed_the_brief_not_the_bare_goal() {
    let (mut daemon, project, _dir) = daemon("delegate-brief");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    // The Goal prints the subagent's own first argument (Unix) or task file
    // (Windows): the brief exactly as the harness received it.
    let goal = if cfg!(windows) {
        "Get-Content -Raw -LiteralPath $env:DISPATCH_TASK_FILE.Trim('\"')"
    } else {
        "printf '%s\\n' \"$1\""
    };
    let caller = ask(&mut daemon, parent, goal);
    let request = pending(&drain(&ui)).expect("asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    let tail = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegateFinished { tail, .. } => {
                Some(String::from_utf8_lossy(tail).into_owned())
            }
            _ => None,
        })
        .expect("finished");
    assert!(
        tail.contains("You are a subagent started by Dispatch"),
        "{tail}"
    );
    assert!(tail.contains("## Report back"), "{tail}");
}

#[test]
fn the_prompt_carries_the_handoff() {
    let (mut daemon, project, _dir) = daemon("delegate-prompt-handoff");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let _caller = ask(&mut daemon, parent, "echo shown");

    let seen = drain(&ui);
    let shown = seen.iter().find_map(|m| match m {
        ServerMessage::DelegatePending {
            handoff,
            task,
            interactive,
            ..
        } => Some((handoff.clone(), task.clone(), *interactive)),
        _ => None,
    });
    let (handoff, task, interactive) = shown.expect("asked");
    let handoff = handoff.expect("the handoff travels");
    assert_eq!(handoff.goal, "echo shown");
    assert_eq!(
        task, handoff.text,
        "an older client is shown the whole handoff"
    );
    assert!(!interactive);
}

#[test]
fn approving_a_request_starts_a_subagent_under_its_parent() {
    let (mut daemon, project, _dir) = daemon("delegate-approve");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo delegated-42");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    let finished = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegateFinished { exit, tail, .. } => Some((*exit, tail.clone())),
            _ => None,
        })
        .expect("the caller is answered");
    assert_eq!(finished.0, 0, "the subagent's own exit code");
    assert!(
        String::from_utf8_lossy(&finished.1).contains("delegated-42"),
        "the tail carries what the subagent printed, got {:?}",
        String::from_utf8_lossy(&finished.1)
    );
    assert_eq!(daemon.pane_count(), 2, "the subagent's pane is kept");
}

#[test]
fn denying_a_request_starts_nothing() {
    let (mut daemon, project, _dir) = daemon("delegate-deny");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo never");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: false,
            blanket: false,
        },
    );

    let seen = drain(&caller);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Denied,
                ..
            }
        )),
        "the caller is told, got {seen:#?}"
    );
    assert_eq!(daemon.pane_count(), 1);
}

#[test]
fn a_blanket_approval_stops_the_asking() {
    let (mut daemon, project, _dir) = daemon("delegate-blanket");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let first = ask(&mut daemon, parent, "echo one");
    let request = pending(&drain(&ui)).expect("the first is asked about");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: true,
        },
    );
    wait_for(&mut daemon, &first, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    let _ = drain(&ui);

    // The second request from the same pane is not put to anyone.
    let second = ask(&mut daemon, parent, "echo two");
    wait_for(&mut daemon, &second, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    assert!(
        pending(&drain(&ui)).is_none(),
        "a pane approved with [A] is not asked about again"
    );
}

#[test]
fn a_subagent_dies_with_the_caller_that_asked_for_it() {
    let (mut daemon, project, _dir) = daemon("delegate-orphan");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, "sleep 30");
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }) && m_is_child(m))
    });

    // The agent hits Ctrl-C, or its pane is killed: either way the socket goes.
    daemon.detach_for_test(9);
    daemon.tick();

    assert_eq!(
        daemon.pane_count(),
        1,
        "a one-off subagent has nobody left to answer"
    );
}

#[test]
fn a_blanket_approved_subagent_survives_its_caller() {
    let (mut daemon, project, _dir) = daemon("delegate-durable");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, "sleep 30");
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: true,
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }) && m_is_child(m))
    });

    daemon.detach_for_test(9);
    daemon.tick();

    assert_eq!(
        daemon.pane_count(),
        2,
        "[A] is how the user says to let this pane's work run"
    );
}

#[test]
fn a_request_nobody_answers_is_expired_when_its_time_is_up() {
    let (mut daemon, project, _dir) = daemon_with_limits(
        "delegate-timeout",
        DelegationLimits {
            request_timeout_secs: 0,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo never");

    daemon.tick();

    let seen = drain(&caller);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Expired { .. },
                ..
            }
        )),
        "a caller must not wait on an unattended daemon forever, got {seen:#?}"
    );
    assert_eq!(daemon.pane_count(), 1, "and a late approval spawns nothing");
}

#[test]
fn a_pane_the_daemon_does_not_own_cannot_delegate() {
    let (mut daemon, _project, _dir) = daemon("delegate-stranger");
    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    let _ = drain(&caller);

    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent: PaneId::new(),
            harness: "shell".into(),
            task: "echo hello".into(),
            size: (80, 24),
            handoff: Some(handoff_for("echo hello")),
            interactive: false,
        },
    );

    assert!(
        matches!(
            drain(&caller).first(),
            Some(ServerMessage::Error {
                error: ProtocolError::NoSuchPane(_)
            })
        ),
        "an unknown parent is not a pane this daemon can attribute work to"
    );
}

/// Round 3, item 2: an approval that cannot reach its caller must not panic
/// indexing the pane it just spawned.
///
/// A budget of zero makes the very next thing sent to a client that already
/// has anything unread `Refused::Behind`, without a real socket or a wait: the
/// caller's own `Welcome` is left sitting in its queue, so `resolve`'s answer
/// to `DelegateResolved` is what trips it. That reaches `hang_up`, then
/// `abandon`, which terminates the one-off subagent `approve` just spawned
/// before `approve` goes on to read its size.
#[test]
fn an_approval_whose_caller_has_hung_up_does_not_panic() {
    let (mut daemon, project, _dir) = daemon("delegate-behind");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    // Past this point, anything already queued and unread makes the next
    // send to that client `Refused::Behind`.
    daemon.set_budgets(Budgets {
        outbox_bytes: 0,
        ..Budgets::default()
    });

    // The caller's own `Welcome`, queued when it said `Hello`, is left
    // unread on purpose -- that is what is still waiting when its approval
    // comes back.
    let _caller = ask(&mut daemon, parent, "echo never");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    // No panic reached here is the assertion; this line only runs at all if
    // `approve` survived indexing the pane `abandon` had already removed.
    assert_eq!(
        daemon.pane_count(),
        1,
        "the one-off subagent was terminated along with its caller, leaving only the parent"
    );
}

#[test]
fn a_delegate_caller_is_not_sent_pane_output() {
    // It waits on one request; the fleet's output is a firehose it never reads.
    // Subscribing here matters: an unsubscribed client is already excluded by
    // `broadcast`, which would let this pass even if the role filter were
    // missing. Subscribing puts the assertion on the role check alone.
    let (mut daemon, project, _dir) = daemon("delegate-quiet");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = ask(&mut daemon, parent, "echo quiet");
    daemon.request_for_test(9, ClientMessage::Subscribe);
    let _ = drain(&caller);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: parent,
            bytes: b"echo noisy\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneOutput { .. }))
    });

    assert!(
        !drain(&caller)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneOutput { .. })),
        "a delegate caller hears about its own request only"
    );
}

#[test]
fn a_delegate_callers_subscribe_catch_up_carries_none_of_the_fleet() {
    // `broadcast` keeps the fleet's ongoing traffic from a delegate caller, but
    // `Subscribe`'s catch-up is a separate path that replays what already
    // happened before this client asked — a pane with history already has
    // something to replay by the time this runs. Building the pane and its
    // history first, and confirming an interface client actually saw the
    // output, is what makes this assertion rest on the role filter rather than
    // on the pane happening to be silent when the delegate caller connects.
    let (mut daemon, project, _dir) = daemon("delegate-catchup-history");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: parent,
            bytes: b"echo history\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneOutput { .. }))
    });

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(9, ClientMessage::Subscribe);

    let seen = drain(&caller);
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneOutput { .. })),
        "a delegate caller's Subscribe catch-up must not replay pane history, got {seen:#?}"
    );
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. })),
        "a delegate caller's Subscribe catch-up must not announce panes either, got {seen:#?}"
    );
}

#[test]
fn a_finished_subagent_survives_its_caller_detaching() {
    // `dispatch delegate` exits the instant it has its answer, so this is the
    // common case, not an edge case: reaping the pane here would throw away
    // the very output TAIL_BYTES exists so a person can still read.
    let (mut daemon, project, _dir) = daemon("delegate-finished-orphan");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo done-42");
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    // The delegate process is gone by now in real use.
    daemon.detach_for_test(9);
    daemon.tick();

    assert_eq!(
        daemon.pane_count(),
        2,
        "a finished subagent is not reaped just because its caller is gone"
    );
}

#[test]
fn closing_the_asking_pane_refuses_its_pending_request() {
    // Without this, the caller's `Pending` entry is gone the moment the
    // decision arrives (there is none to time out), and `approve`'s missing-
    // pane branch used to return silently: the caller would hang until the
    // daemon itself died.
    let (mut daemon, project, _dir) = daemon("delegate-parent-closed");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo never");
    let _request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: parent });

    let seen = drain(&caller);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Refused { .. },
                ..
            }
        )),
        "a caller must not hang forever on a pane that closed before answering, got {seen:#?}"
    );
}

/// Ends `pane`'s shell, and ticks until its exit has been reported.
fn exit_pane(daemon: &mut Daemon, ui: &Inbox, pane: PaneId) {
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"exit 0\r".to_vec(),
        },
    );
    wait_for(daemon, ui, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::PaneChanged {
                    pane: p,
                    update: PaneUpdate::Status {
                        status: PaneStatus::Exited(_)
                    },
                } if *p == pane
            )
        })
    });
}

/// Why a caller's request was refused, if it was.
fn refusal(messages: &[ServerMessage]) -> Option<String> {
    messages.iter().find_map(|m| match m {
        ServerMessage::DelegateResolved {
            outcome: dispatch_proto::DelegateOutcome::Refused { reason },
            ..
        } => Some(reason.clone()),
        _ => None,
    })
}

#[test]
fn approving_a_request_from_a_pane_that_has_exited_starts_nothing() {
    // An exited pane keeps its row, so its last output can be read, but the
    // agent that asked is gone: a subagent started for it would work for
    // nobody, under a parent nothing will ever close.
    let (mut daemon, project, _dir) = daemon("delegate-parent-exited");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo never");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    exit_pane(&mut daemon, &ui, parent);
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = drain(&caller);
    assert_eq!(
        refusal(&seen).as_deref(),
        Some("the pane that asked has exited"),
        "the caller is told why, got {seen:#?}"
    );
    assert_eq!(daemon.pane_count(), 1, "no subagent was started");
}

#[test]
fn a_blanket_approval_starts_nothing_for_a_pane_that_has_exited() {
    // The blanket outlives the agent it was given to, since the row does;
    // what it approved was that agent's requests, and there are no more.
    let (mut daemon, project, _dir) = daemon("delegate-blanket-exited");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let first = ask(&mut daemon, parent, "echo one");
    let request = pending(&drain(&ui)).expect("the first is asked about");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: true,
        },
    );
    wait_for(&mut daemon, &first, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    exit_pane(&mut daemon, &ui, parent);
    let second = ask(&mut daemon, parent, "echo two");

    let seen = drain(&second);
    assert_eq!(
        refusal(&seen).as_deref(),
        Some("the pane that asked has exited"),
        "the caller is told why, got {seen:#?}"
    );
    assert_eq!(
        daemon.pane_count(),
        2,
        "the parent and its first subagent, and nothing started since"
    );
}

#[test]
fn every_interface_client_is_told_when_a_request_is_resolved() {
    // The next task draws the prompt on every interface client that saw it;
    // without this, a denied or expired request stays on screen for everyone
    // but the one who answered it.
    let (mut daemon, project, _dir) = daemon("delegate-resolved-broadcast");
    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let second = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &first, project);
    let _ = drain(&second);

    let caller = ask(&mut daemon, parent, "echo resolved");
    let request = pending(&drain(&first)).expect("the first client sees the prompt");
    assert!(
        pending(&drain(&second)).is_some(),
        "the second interface client sees the same prompt"
    );

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: false,
            blanket: false,
        },
    );
    let _ = drain(&caller);

    let seen = drain(&second);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Denied,
                ..
            }
        )),
        "an interface client that saw the prompt should be told it is resolved, got {seen:#?}"
    );
}

#[test]
fn a_delegate_caller_that_subscribes_is_not_told_about_pending_requests() {
    // The Subscribe catch-up is for interface clients drawing the fleet; a
    // delegate caller does not draw prompts, and `broadcast` already excludes
    // it for the same reason once a request is live.
    let (mut daemon, project, _dir) = daemon("delegate-catchup");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = ask(&mut daemon, parent, "echo catchup");
    let _ = drain(&caller);

    daemon.request_for_test(9, ClientMessage::Subscribe);

    assert!(
        !drain(&caller)
            .iter()
            .any(|m| matches!(m, ServerMessage::DelegatePending { .. })),
        "a delegate caller's own Subscribe catch-up must not include prompts"
    );
}

#[test]
fn closing_a_running_subagents_pane_answers_its_caller() {
    // The pane is killed rather than allowed to finish, so pump_panes never
    // sees its exit, and the request was already removed from `pending` when
    // it was approved -- so without an explicit answer here, nothing would
    // ever tell the caller anything.
    let (mut daemon, project, _dir) = daemon("delegate-close-running-subagent");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "sleep 30");
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::DelegateResolved {
                    outcome: dispatch_proto::DelegateOutcome::Approved { .. },
                    ..
                }
            )
        })
    });
    let subagent = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Approved { pane },
                ..
            } => Some(*pane),
            _ => None,
        })
        .expect("the subagent was approved");

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: subagent });

    let seen = drain(&caller);
    assert!(
        seen.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. })),
        "closing a running subagent's own pane must still answer its caller, got {seen:#?}"
    );
}

#[test]
fn an_interface_client_that_delegates_is_not_told_twice() {
    // Nothing gates DelegateRequest on role, so an interface client can be its
    // own caller -- an agent delegating from a pane someone happens to be
    // watching through the same connection. `resolve` answers it directly;
    // the broadcast half must not repeat that answer.
    let (mut daemon, project, _dir) = daemon("delegate-self-caller");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _ = drain(&ui);

    daemon.request_for_test(
        1,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: "echo self".into(),
            size: (80, 24),
            handoff: Some(handoff_for("echo self")),
            interactive: false,
        },
    );
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: false,
            blanket: false,
        },
    );

    let seen = drain(&ui);
    let resolved = seen
        .iter()
        .filter(|m| matches!(m, ServerMessage::DelegateResolved { .. }))
        .count();
    assert_eq!(
        resolved, 1,
        "an interface client that is also the caller should hear its answer once, got {seen:#?}"
    );
}

#[test]
fn a_harness_with_an_empty_task_form_is_refused_without_asking() {
    // The point of matching approve()'s own predicate: the user is never put
    // in the position of approving something that will just fail afterward.
    let (mut daemon, project, _dir) = daemon("delegate-empty-task-args");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _ = drain(&ui);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    let _ = drain(&caller);
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "no-task-args".into(),
            task: "echo never".into(),
            size: (80, 24),
            handoff: Some(handoff_for("echo never")),
            interactive: false,
        },
    );

    let seen = drain(&caller);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Refused { .. },
                ..
            }
        )),
        "a harness whose [task] has no runnable args must be refused immediately, got {seen:#?}"
    );
    assert!(
        pending(&drain(&ui)).is_none(),
        "the user must never be asked about a harness that cannot actually run"
    );
}

#[test]
fn closing_a_pane_drops_its_whole_delegation_subtree() {
    // A grandchild must go too, not just the direct child: otherwise it is
    // left with a `parent` pointing at nothing, and depth_of/live_children
    // silently under-count from then on.
    let (mut daemon, project, _dir) = daemon_with_limits(
        "delegate-cascade",
        DelegationLimits {
            max_depth: 2,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let root = spawn_pane_for_test(&mut daemon, &ui, project);

    let first_caller = ask_as(&mut daemon, 9, root, "sleep 30");
    let request = pending(&drain(&ui)).expect("the interface is asked about the child");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    let seen = wait_for(&mut daemon, &first_caller, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::DelegateResolved {
                    outcome: dispatch_proto::DelegateOutcome::Approved { .. },
                    ..
                }
            )
        })
    });
    let child = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::DelegateResolved {
                outcome: dispatch_proto::DelegateOutcome::Approved { pane },
                ..
            } => Some(*pane),
            _ => None,
        })
        .expect("the child was approved");
    let _ = drain(&ui);

    // A second delegate caller, as if the child's own agent asked for a
    // subagent of its own.
    let second_caller = ask_as(&mut daemon, 10, child, "sleep 30");
    let request = pending(&drain(&ui)).expect("the interface is asked about the grandchild");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    wait_for(&mut daemon, &second_caller, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::DelegateResolved {
                    outcome: dispatch_proto::DelegateOutcome::Approved { .. },
                    ..
                }
            )
        })
    });

    assert_eq!(
        daemon.pane_count(),
        3,
        "root, child, and grandchild all exist"
    );

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: root });

    assert_eq!(
        daemon.pane_count(),
        0,
        "closing the root must drop the whole subtree, not just its direct child"
    );
}

#[test]
fn a_prompt_whose_caller_has_gone_is_withdrawn_rather_than_left_on_screen() {
    // Ctrl-C on `dispatch delegate` closes the socket, and the request goes
    // with it. Dropped in silence, the prompt stayed on every interface client:
    // the user presses `a`, `DelegateDecision` finds no pending entry, returns,
    // and nothing whatsoever happens. Every other resolution path broadcasts,
    // and a withdrawal is exactly what closes a prompt.
    let (mut daemon, project, _dir) = daemon("delegate-caller-gone");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, "echo never");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.detach_for_test(9);

    let seen = drain(&ui);
    assert!(
        seen.iter().any(|m| matches!(
            m,
            ServerMessage::DelegateResolved {
                request: withdrawn,
                outcome: dispatch_proto::DelegateOutcome::Refused { .. },
            } if *withdrawn == request
        )),
        "the prompt must be withdrawn from the interface, got {seen:#?}"
    );

    // And answering it afterwards is answering nothing, which is precisely why
    // it must not still be on screen.
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    daemon.tick();
    assert_eq!(
        daemon.pane_count(),
        1,
        "a withdrawn request cannot be approved into a subagent"
    );
}

#[test]
fn a_late_subscriber_is_told_about_pending_requests_oldest_first() {
    // The client documents its queue as oldest first and shows the front of it;
    // `HashMap` order would hand a reattaching client the prompts in whatever
    // order the hasher happened to like, so the request the user has been
    // waiting on longest need not be the one they are shown.
    let (mut daemon, project, _dir) = daemon("delegate-catch-up-order");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    // Six, because one hash order in a handful agreeing with insertion order is
    // luck; six agreeing is not.
    let mut asked = Vec::new();
    for (index, id) in (20..26).enumerate() {
        let _caller = ask_as(&mut daemon, id, parent, &format!("task {index}"));
        asked.push(
            pending(&drain(&ui)).unwrap_or_else(|| panic!("the interface is asked about {index}")),
        );
    }

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let replayed: Vec<dispatch_core::RequestId> = drain(&late)
        .iter()
        .filter_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .collect();

    assert_eq!(
        replayed, asked,
        "a late subscriber should be caught up in the order the requests were asked"
    );
}

#[test]
fn closing_an_empty_project_forgets_it_and_tells_everyone() {
    let (mut daemon, project, _dir) = daemon("close-project");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(1, ClientMessage::CloseProject { project });

    assert!(
        daemon.projects().is_empty(),
        "the daemon forgets it, or the next Subscribe hands it straight back"
    );
    assert!(
        drain(&inbox)
            .iter()
            .any(|m| matches!(m, ServerMessage::ProjectClosed { project: p } if *p == project)),
        "closing should be broadcast"
    );
}

#[test]
fn a_project_with_panes_is_not_closed() {
    // Its agents belong to the daemon and would carry on running with nothing
    // left to reach them by.
    let (mut daemon, project, _dir) = daemon("close-project-busy");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    daemon.request_for_test(1, ClientMessage::CloseProject { project });

    assert_eq!(daemon.projects().len(), 1, "it stays");
    assert!(
        drain(&inbox)
            .iter()
            .any(|m| matches!(m, ServerMessage::Error { .. })),
        "and the client is told why"
    );
}

#[test]
fn closing_an_unknown_project_is_reported() {
    let (mut daemon, _, _dir) = daemon("close-project-unknown");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::CloseProject {
            project: ProjectId::new(),
        },
    );

    assert!(drain(&inbox).iter().any(|m| matches!(
        m,
        ServerMessage::Error {
            error: ProtocolError::NoSuchProject(_)
        }
    )));
}

/// A task that is still running thirty seconds from now, on every platform.
///
/// `sleep` is not a command under `cmd.exe`: there it fails at once, and a
/// test about what is *running* would pass for the wrong reason. No `>nul`:
/// from Task 17 the Windows fixture runs its task under PowerShell, where
/// that redirection fails.
fn long_task() -> &'static str {
    if cfg!(windows) {
        "ping -n 30 127.0.0.1"
    } else {
        "sleep 30"
    }
}

/// Every `DelegateResolved` outcome among `messages`.
fn outcomes(messages: &[ServerMessage]) -> Vec<DelegateOutcome> {
    messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::DelegateResolved { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn approving_more_requests_than_the_cap_allows_starts_only_what_fits() {
    // Both requests are asked about while nothing is running, so both pass
    // the check on arrival. Approving them one after the other must still
    // start only one: the cap is on what runs, not on what is asked.
    let (mut daemon, project, _dir) = daemon_with_limits(
        "cap-at-approval",
        DelegationLimits {
            max_depth: 1,
            max_live_per_parent: 1,
            request_timeout_secs: 600,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let first = ask_as(&mut daemon, 8, parent, long_task());
    let second = ask_as(&mut daemon, 9, parent, long_task());
    let requests: Vec<RequestId> = drain(&ui)
        .iter()
        .filter_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 2, "both fit while nothing runs yet");

    for request in requests {
        daemon.request_for_test(
            1,
            ClientMessage::DelegateDecision {
                request,
                approve: true,
                blanket: false,
            },
        );
    }

    assert_eq!(
        daemon.pane_count(),
        2,
        "the parent and exactly one subagent"
    );

    let mut told = outcomes(&drain(&first));
    told.extend(outcomes(&drain(&second)));
    assert_eq!(
        told.iter()
            .filter(|o| matches!(o, DelegateOutcome::Approved { .. }))
            .count(),
        1,
        "one caller is told it runs: {told:?}"
    );
    assert!(
        told.iter()
            .any(|o| matches!(o, DelegateOutcome::Refused { reason } if reason.contains("cap"))),
        "the other is told why it does not: {told:?}"
    );
}

#[test]
fn approvals_from_two_interfaces_do_not_share_one_slot() {
    // The same race with the approvals coming from two people at two
    // screens, which is the ordinary shape on a shared fleet.
    let (mut daemon, project, _dir) = daemon_with_limits(
        "cap-two-uis",
        DelegationLimits {
            max_depth: 1,
            max_live_per_parent: 1,
            request_timeout_secs: 600,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let other_ui = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _ = drain(&other_ui);

    let _first = ask_as(&mut daemon, 8, parent, long_task());
    let _second = ask_as(&mut daemon, 9, parent, long_task());
    let requests: Vec<RequestId> = drain(&ui)
        .iter()
        .filter_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .collect();

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request: requests[0],
            approve: true,
            blanket: false,
        },
    );
    daemon.request_for_test(
        2,
        ClientMessage::DelegateDecision {
            request: requests[1],
            approve: true,
            blanket: false,
        },
    );

    assert_eq!(
        daemon.pane_count(),
        2,
        "the parent and exactly one subagent"
    );
}

#[test]
fn a_blanket_approved_pane_at_its_cap_is_refused_rather_than_started() {
    let (mut daemon, project, _dir) = daemon_with_limits(
        "cap-blanket",
        DelegationLimits {
            max_depth: 1,
            max_live_per_parent: 1,
            request_timeout_secs: 600,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let _first = ask_as(&mut daemon, 8, parent, long_task());
    let request = pending(&drain(&ui)).expect("the first is asked about");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: true,
        },
    );

    let second = ask_as(&mut daemon, 9, parent, long_task());

    assert_eq!(
        daemon.pane_count(),
        2,
        "blanket approval is not a second slot"
    );
    assert!(
        outcomes(&drain(&second))
            .iter()
            .any(|o| matches!(o, DelegateOutcome::Refused { .. })),
        "the second caller is refused"
    );
}

fn spawn_request(project: ProjectId) -> ClientMessage {
    ClientMessage::SpawnPane {
        project,
        harness: "shell".into(),
        size: (80, 24),
        place: Placement::Auto,
        settings: Default::default(),
    }
}

#[test]
fn nothing_is_acted_on_before_a_hello() {
    let (mut daemon, project, _dir) = daemon("before-hello");
    let inbox = daemon.attach_for_test(1);

    daemon.request_for_test(1, spawn_request(project));

    assert_eq!(
        daemon.pane_count(),
        0,
        "a request before Hello starts nothing"
    );
    assert!(
        drain(&inbox)
            .iter()
            .any(|m| matches!(m, ServerMessage::Error { .. })),
        "and says why"
    );

    // The connection is over: a Hello now is too late to rescue it.
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, spawn_request(project));
    assert_eq!(daemon.pane_count(), 0);
    assert!(drain(&inbox).is_empty(), "nothing more reaches it");
}

#[test]
fn a_refused_client_cannot_act_afterwards() {
    // The audit's probe: an incompatible Hello is answered with an error, and
    // then a SpawnPane from the same client started a pane anyway.
    let (mut daemon, project, _dir) = daemon("refused-acts");
    let refused = daemon.attach_for_test(2);
    daemon.request_for_test(
        2,
        ClientMessage::Hello {
            version: dispatch_proto::Version {
                major: 99,
                minor: 0,
            },
            client: "incompatible".into(),
            role: dispatch_proto::Role::Interface,
        },
    );
    assert!(matches!(
        refused.try_recv(),
        Ok(ServerMessage::Error {
            error: ProtocolError::IncompatibleVersion { .. }
        })
    ));

    daemon.request_for_test(2, spawn_request(project));

    assert_eq!(daemon.pane_count(), 0, "a refused client spawned a pane");
}

#[test]
fn a_detached_client_cannot_act() {
    let (mut daemon, project, _dir) = daemon("detached-acts");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.detach_for_test(1);

    // A request already read off the socket arrives after the reader said
    // the client left: events from one client are ordered, but a request
    // queued by a thread that is gone is still a request from nobody.
    daemon.request_for_test(1, spawn_request(project));

    assert_eq!(daemon.pane_count(), 0);
}

/// Unix only, although `stall` has a Windows form: no program on Windows
/// can stop its pane's input being read. The pipe a pane's input goes down
/// is read by the pseudoconsole host, not by the program in the console,
/// and the host moves what arrives into the console's input buffer whether
/// or not anything reads that. The queue this test fills drains instead,
/// and the refusal it ends on -- input past the budget, waiting for a pane
/// that is not reading -- cannot be brought about.
#[test]
#[cfg(unix)]
fn a_stalled_pane_does_not_stall_the_daemon() {
    let (mut daemon, project, _dir) = daemon_with_limits(
        "stalled",
        DelegationLimits {
            request_timeout_secs: 1,
            ..DelegationLimits::default()
        },
    );
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);

    // A shell whose output must keep flowing, and a request whose deadline
    // must keep counting, while another pane is stalled.
    let shell = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, shell, "echo never");

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "stall".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    let seen = wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { harness, .. } if harness == "stall"))
    });
    let stalled = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, harness, .. } if harness == "stall" => Some(*pane),
            _ => None,
        })
        .expect("the stalled pane was spawned");
    wait_for(&mut daemon, &ui, |m| {
        output_of(m, stalled).contains("READY")
    });

    // The audit's probe: this one request held the loop for as long as the
    // pane went on not reading.
    let started = Instant::now();
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: stalled,
            bytes: vec![b'x'; 2 * 1024 * 1024],
        },
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "one write held the daemon for {:?}",
        started.elapsed()
    );

    // Another client is still answered.
    let other = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    let _ = drain(&other);
    daemon.request_for_test(2, ClientMessage::Ping { token: 5 });
    assert_eq!(drain(&other), vec![ServerMessage::Pong { token: 5 }]);

    // Another pane's output still flows.
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: shell,
            bytes: b"echo still-flowing\n".to_vec(),
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        output_of(m, shell).contains("still-flowing")
    });

    // The pending request still runs out of time.
    wait_for(&mut daemon, &caller, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::DelegateResolved {
                    outcome: DelegateOutcome::Expired { .. },
                    ..
                }
            )
        })
    });

    // More than the budget is refused out loud rather than queued.
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane: stalled,
            bytes: vec![b'y'; dispatch_pty::INPUT_BUDGET],
        },
    );
    let told = drain(&ui);
    assert!(
        told.iter().any(|m| matches!(
            m,
            ServerMessage::Error { error: ProtocolError::Other(text) } if text.contains("not reading")
        )),
        "the writer is told its input was dropped, got {told:#?}"
    );

    // Closing the stalled pane takes effect at once.
    let started = Instant::now();
    daemon.request_for_test(1, ClientMessage::ClosePane { pane: stalled });
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(daemon.pane_count(), 1, "only the shell is left");

    // And the daemon still stops when asked.
    daemon.shutdown_handle().request();
    let started = Instant::now();
    daemon.run();
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A client that says `Ping` for ever, counting the frames it has begun.
struct EndlessPings {
    frame: Vec<u8>,
    at: usize,
    begun: Arc<AtomicUsize>,
}

impl Read for EndlessPings {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at == 0 {
            self.begun.fetch_add(1, Ordering::Relaxed);
        }
        let n = buf.len().min(self.frame.len() - self.at);
        buf[..n].copy_from_slice(&self.frame[self.at..self.at + n]);
        self.at = (self.at + n) % self.frame.len();
        Ok(n)
    }
}

/// Waits for `count` to reach `floor` and then stop moving, and says where
/// it stopped.
///
/// Stillness alone could be a thread the scheduler set aside for a moment
/// on a loaded machine; the floor rules that out. Gives up after ten
/// seconds with wherever it has got to: a count that never reaches the
/// floor, or never stops past it, is what the caller asserts against, not
/// a hang.
fn settled(count: &AtomicUsize, floor: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = count.load(Ordering::Relaxed);
    let mut still_since = Instant::now();

    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        let now = count.load(Ordering::Relaxed);
        if now != last {
            last = now;
            still_since = Instant::now();
        } else if last >= floor && still_since.elapsed() >= Duration::from_millis(300) {
            break;
        }
    }

    last
}

#[test]
fn a_client_outrunning_a_busy_loop_waits_at_the_event_backlog() {
    // The loop is never run here, which is how a loop busy with something
    // else looks to a client's reader thread. With nothing taking events,
    // the reader may queue EVENT_BACKLOG of them -- its attach and then its
    // requests -- and must then wait to hand over the one it has just read,
    // reading nothing further. Unbounded, a client sending faster than the
    // daemon acts would be queued for without limit.
    let (daemon, _, _dir) = daemon("backlog");

    let mut frame = Vec::new();
    Frame::write(&mut frame, &ClientMessage::Ping { token: 1 }).expect("a ping encodes");
    let begun = Arc::new(AtomicUsize::new(0));
    let reader = EndlessPings {
        frame,
        at: 0,
        begun: Arc::clone(&begun),
    };
    let connection = Connection::from_halves(Box::new(reader), Box::new(std::io::sink()));
    spawn_client(
        1,
        connection,
        &daemon.sender,
        &Arc::new(AtomicUsize::new(0)),
    );

    // The attach takes a place, so EVENT_BACKLOG - 1 requests are queued and
    // one more has been read and is waiting to join them.
    assert_eq!(
        settled(&begun, EVENT_BACKLOG),
        EVENT_BACKLOG,
        "the reader should stop once the backlog is full"
    );

    // One place freed lets exactly one more through: the reader was waiting
    // on the backlog, not finished or stuck on anything else.
    assert!(
        matches!(daemon.events.try_recv(), Ok(Event::Attached(1, _))),
        "the attach is first in the queue"
    );
    assert_eq!(
        settled(&begun, EVENT_BACKLOG + 1),
        EVENT_BACKLOG + 1,
        "one place freed should let one more request in"
    );
}

use std::io::{Read, Write};

/// A daemon serving a real endpoint on a thread of its own, stopped when
/// dropped.
///
/// Most tests here drive the loop directly; these are the ones about what
/// happens to the connection itself, which only a socket can show.
struct Served {
    endpoint: PathBuf,
    project: ProjectId,
    shutdown: Shutdown,
    thread: Option<std::thread::JoinHandle<()>>,
    _dir: TempDir,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.shutdown.request();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Serves a fresh daemon under `budgets`. Keep `label` short: a Unix
/// socket's whole path must fit in about a hundred bytes.
fn served(label: &str, budgets: Budgets) -> Served {
    let (mut daemon, project, dir) = daemon(label);
    daemon.set_budgets(budgets);

    let endpoint = dir.0.join("d.sock");
    let listener = Listener::bind_to(&endpoint).expect("binding succeeds");
    let shutdown = daemon.shutdown_handle();
    let thread = std::thread::spawn(move || {
        let _ = daemon.serve(listener);
    });

    Served {
        endpoint,
        project,
        shutdown,
        thread: Some(thread),
        _dir: dir,
    }
}

type RawReader = Box<dyn Read + Send>;
type RawWriter = Box<dyn Write + Send>;

fn raw_client(endpoint: &Path) -> (RawReader, RawWriter) {
    Connection::connect_to(endpoint)
        .expect("the daemon is listening")
        .split()
}

/// Whether writes to the daemon start failing within `patience` -- that is,
/// whether the daemon has let go of the half it reads from.
fn stops_listening(mut writer: RawWriter, patience: Duration) -> bool {
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        if Frame::write(&mut writer, &ClientMessage::Ping { token: 0 }).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Connects as a well-behaved interface client and collects what arrives in
/// `window`.
fn subscribe_and_collect(endpoint: &Path, window: Duration) -> Vec<ServerMessage> {
    subscribe_and_collect_until(endpoint, window, |_| false)
}

/// Connects as a well-behaved interface client and collects what arrives
/// until `done` holds of it, or `window` has passed.
fn subscribe_and_collect_until(
    endpoint: &Path,
    window: Duration,
    done: impl Fn(&[ServerMessage]) -> bool,
) -> Vec<ServerMessage> {
    let (mut reader, mut writer) = raw_client(endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");
    Frame::write(&mut writer, &ClientMessage::Subscribe).expect("writing succeeds");

    let (heard, hearing) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(message) = Frame::read::<_, ServerMessage>(&mut reader) {
            if heard.send(message).is_err() {
                return;
            }
        }
    });

    let deadline = Instant::now() + window;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match hearing.recv_timeout(left) {
            Ok(message) => seen.push(message),
            Err(_) => break,
        }
        if done(&seen) {
            break;
        }
    }
    drop(writer);
    seen
}

/// The output bytes among `messages`, however many panes they came from.
///
/// Used only by the budget tests below, which are `#[cfg(unix)]`: without
/// the same gate here, a Windows build has nothing left that calls it.
#[cfg(unix)]
fn output_bytes(messages: &[ServerMessage]) -> usize {
    messages
        .iter()
        .map(|m| match m {
            ServerMessage::PaneOutput { bytes, .. } => bytes.len(),
            _ => 0,
        })
        .sum()
}

/// How long a test waits for something the daemon is expected to do about a
/// client -- hang up on it, or admit the next one.
///
/// Not itself past every budget a test sets: the default `handshake` is
/// exactly this long. A test that cares about a different deadline and
/// must rule the handshake one out raises it clear of `PATIENCE` instead,
/// so a failure cannot be mistaken for the wrong cause.
const PATIENCE: Duration = Duration::from_secs(10);

#[test]
#[cfg(unix)]
fn a_client_that_never_reads_costs_no_more_than_its_budget() {
    const BUDGET: usize = 256 * 1024;
    let (mut daemon, project, _dir) = daemon("unread");
    daemon.set_budgets(Budgets {
        outbox_bytes: BUDGET,
        ..Budgets::default()
    });

    let reading = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let never_reads = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "flood".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    // Two megabytes reach the client that reads. The other one's queue
    // stops growing at the budget, instead of holding all two.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut delivered = 0;
    while delivered < 2 * 1024 * 1024 {
        assert!(
            Instant::now() < deadline,
            "the flood stopped reaching the client that reads ({delivered} bytes)"
        );
        daemon.tick();
        delivered += output_bytes(&drain(&reading));
        std::thread::sleep(Duration::from_millis(5));
    }

    let backlog = output_bytes(&drain(&never_reads));
    assert!(
        backlog <= BUDGET + dispatch_pty::DRAIN_BUDGET + 8192,
        "{backlog} bytes were queued for a client that never read"
    );
    assert!(
        matches!(
            never_reads.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ),
        "the daemon let go of it"
    );
}

/// Every byte of pane output among `messages`, in the order it arrived.
#[cfg(unix)]
fn pane_output(messages: &[ServerMessage]) -> Vec<u8> {
    messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::PaneOutput { bytes, .. } => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

/// `bytes` without the carriage returns a terminal puts before each newline.
#[cfg(unix)]
fn without_returns(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|&b| b != b'\r').collect()
}

/// What the `count` harness prints, carriage returns aside.
#[cfg(unix)]
fn counted() -> Vec<u8> {
    (1..=COUNTED)
        .map(|n| format!("{n}\n"))
        .collect::<String>()
        .into_bytes()
}

#[test]
#[cfg(unix)]
fn a_client_that_stops_reading_is_hung_up_and_can_come_back() {
    let served = served(
        "stop-read",
        Budgets {
            outbox_bytes: 256 * 1024,
            ..Budgets::default()
        },
    );

    let (_reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");
    Frame::write(&mut writer, &ClientMessage::Subscribe).expect("writing succeeds");
    Frame::write(
        &mut writer,
        &ClientMessage::SpawnPane {
            project: served.project,
            harness: "count".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    )
    .expect("writing succeeds");

    // Never read: the socket fills, the outbox passes its budget, and the
    // daemon hangs up -- both halves, so this side's writes start failing.
    assert!(
        stops_listening(writer, Duration::from_secs(20)),
        "a client that stopped reading is still connected"
    );

    // Coming back is an ordinary late subscription: the pane is described,
    // and what was missed is replayed -- as much as the daemon keeps, in the
    // order it was printed, with the pane's next output carrying on from
    // exactly where the replay stops.
    let printed = counted();
    let last = format!("{COUNTED}\n");
    // Only the newest messages are looked at: the whole output, gathered
    // afresh for every message, is quadratic in a replay this size.
    let seen = subscribe_and_collect_until(&served.endpoint, Duration::from_secs(20), |m| {
        let start = m.len().saturating_sub(4);
        without_returns(&pane_output(&m[start..])).ends_with(last.as_bytes())
    });
    assert!(
        seen.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { harness, .. } if harness == "count")),
        "the reconnected client is told about the pane"
    );

    let replay = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneOutput { bytes, .. } => Some(bytes.len()),
            _ => None,
        })
        .expect("the pane's history is replayed");
    let received = without_returns(&pane_output(&seen));
    assert!(
        received.ends_with(last.as_bytes()),
        "the pane's output stopped arriving after {} bytes",
        received.len()
    );
    assert!(
        received.len() <= printed.len() && printed.ends_with(&received),
        "the {} bytes a late client was sent are not the last {} the pane printed, in order; \
         the first to differ is at {:?}",
        received.len(),
        received.len(),
        received
            .iter()
            .rev()
            .zip(printed.iter().rev())
            .position(|(got, want)| got != want)
            .map(|from_end| received.len() - 1 - from_end)
    );
    assert!(
        replay == crate::pane::HISTORY_BYTES || received == printed,
        "the replay held back part of the history: {replay} bytes of {}",
        crate::pane::HISTORY_BYTES
    );
}

#[test]
#[cfg(unix)]
fn a_late_subscriber_is_not_hung_up_for_the_replay_it_asked_for() {
    // Small enough that a single pane's full history is several times
    // over it -- the point of the test is that the replay is not judged
    // against this budget at all.
    const BUDGET: usize = 64 * 1024;
    let (mut daemon, project, _dir) = daemon("late-subscribe");
    daemon.set_budgets(Budgets {
        outbox_bytes: BUDGET,
        ..Budgets::default()
    });

    // A reading client keeps the flood's output moving so pump_panes keeps
    // draining it, until the pane's history -- replayed whole to whoever
    // subscribes next -- is full.
    let producer = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "flood".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut delivered = 0;
    while delivered < 256 * 1024 {
        assert!(
            Instant::now() < deadline,
            "the flood did not fill the pane's history ({delivered} bytes)"
        );
        daemon.tick();
        delivered += output_bytes(&drain(&producer));
        std::thread::sleep(Duration::from_millis(5));
    }

    // Subscribing now asks for that whole history in one reply -- many
    // times the live-traffic budget above. It must be delivered rather
    // than refused: it is what was asked for, not live traffic.
    let subscriber = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    // One tick before anything is drained, with the whole reply still
    // sitting unread: this is exactly the moment a live broadcast used to
    // see the reply's own bulk as backlog and hang the client up for it
    // (round 1, finding 1) -- ticking here, rather than draining first,
    // gives that bug a real chance to happen before this test would ever
    // notice.
    daemon.tick();

    // Drained together, and not counted below: both are what was asked
    // for, or arrived before this client had a chance to read anything --
    // not the ongoing live traffic the loop below measures.
    let replay = drain(&subscriber);
    assert!(
        output_bytes(&replay) >= 256 * 1024,
        "the replay should have carried the pane's full history, got {} bytes",
        output_bytes(&replay)
    );
    assert!(
        !matches!(
            subscriber.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ),
        "the daemon hung up on the subscriber before it had read anything"
    );

    // Reading in lock-step, like any subscribed client, it goes on being
    // sent LIVE output well past the live-traffic budget above -- proving
    // that traffic, arriving over time, is not eaten into by the replay
    // already delivered -- and is never hung up, for as long as it reads.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut total = 0;
    while total < 3 * BUDGET {
        assert!(
            Instant::now() < deadline,
            "the subscriber stopped receiving output ({total} bytes)"
        );
        daemon.tick();
        total += output_bytes(&drain(&subscriber));
        assert!(
            !matches!(
                subscriber.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            ),
            "the daemon hung up on a client that was reading"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_client_that_never_reads_its_own_replies_is_hung_up() {
    // Small enough that a loop of tiny replies -- nothing the fleet
    // produced on its own -- passes it well within a handful of iterations.
    const BUDGET: usize = 2 * 1024;
    let (mut daemon, _project, _dir) = daemon("never-reads-replies");
    daemon.set_budgets(Budgets {
        outbox_bytes: BUDGET,
        ..Budgets::default()
    });

    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    // Never drained: every `Pong` piles up in what this client asked for,
    // which is bounded too -- or a client could grow its queue forever just
    // by asking, without the fleet doing anything at all.
    for token in 0..500 {
        daemon.request_for_test(1, ClientMessage::Ping { token });
    }

    // Whatever was queued before the hang-up is still there to read; once
    // it runs out, the channel is gone rather than merely empty.
    let _ = drain(&inbox);
    assert!(
        matches!(
            inbox.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ),
        "a client that never read its own replies should have been hung up"
    );
}

#[test]
#[cfg(unix)]
fn a_refused_client_is_told_why_before_the_connection_ends() {
    let served = served("refuse-why", Budgets::default());

    let (mut reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(
        &mut writer,
        &ClientMessage::Hello {
            version: dispatch_proto::Version {
                major: 99,
                minor: 0,
            },
            client: "from the future".into(),
            role: dispatch_proto::Role::Interface,
        },
    )
    .expect("writing succeeds");

    // The reason arrives first...
    let reason = Frame::read::<_, ServerMessage>(&mut reader)
        .expect("the daemon answers with why before closing");
    assert!(
        matches!(
            reason,
            ServerMessage::Error {
                error: ProtocolError::IncompatibleVersion { .. }
            }
        ),
        "expected an IncompatibleVersion error, got {reason:?}"
    );

    // ...and only then does the connection itself end, rather than an
    // immediate close racing the write of the refusal and the peer seeing
    // a bare disconnect instead.
    let (done, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(Frame::read::<_, ServerMessage>(&mut reader).is_err());
    });
    assert!(
        ended.recv_timeout(Duration::from_secs(2)).unwrap_or(false),
        "the connection did not end after the refusal"
    );
}

/// Same guarantee as the test above, under real contention.
///
/// One connection at a time essentially never catches the race a review
/// found in `hang_up`'s immediate close: the write of a small refusal all
/// but always wins against a freshly spawned thread. A handful refused at
/// once give the daemon's per-connection close threads real scheduler
/// contention to race the writer threads under, which reliably does catch
/// it (more at once risks the transport's own connection-pairing limits
/// instead, which are not what this is testing).
#[test]
#[cfg(unix)]
fn many_refused_clients_are_all_told_why_before_the_connection_ends() {
    let served = std::sync::Arc::new(served("refuse-many", Budgets::default()));

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let served = std::sync::Arc::clone(&served);
            std::thread::spawn(move || {
                let (mut reader, mut writer) = raw_client(&served.endpoint);
                Frame::write(
                    &mut writer,
                    &ClientMessage::Hello {
                        version: dispatch_proto::Version {
                            major: 99,
                            minor: 0,
                        },
                        client: "from the future".into(),
                        role: dispatch_proto::Role::Interface,
                    },
                )
                .expect("writing succeeds");

                let reason = Frame::read::<_, ServerMessage>(&mut reader);
                assert!(
                    matches!(
                        reason,
                        Ok(ServerMessage::Error {
                            error: ProtocolError::IncompatibleVersion { .. }
                        })
                    ),
                    "connection {i}: expected an IncompatibleVersion error, got {reason:?}"
                );
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e));
    }
}

/// Whether the daemon ends the connection `reader` reads from within
/// `patience`, discarding whatever arrives first.
fn hung_up(mut reader: RawReader, patience: Duration) -> bool {
    let (ended, end) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while Frame::read::<_, ServerMessage>(&mut reader).is_ok() {}
        let _ = ended.send(());
    });
    end.recv_timeout(patience).is_ok()
}

#[test]
fn a_refused_client_is_hung_up_on_and_its_pipelined_request_ignored() {
    // A Hello the daemon refuses, with a SpawnPane right behind it in the
    // same burst: the shape that got a pane started for a refused client.
    let served = served("refused", Budgets::default());
    let (mut reader, mut writer) = raw_client(&served.endpoint);

    Frame::write(
        &mut writer,
        &ClientMessage::Hello {
            version: dispatch_proto::Version {
                major: 99,
                minor: 0,
            },
            client: "future".into(),
            role: dispatch_proto::Role::Interface,
        },
    )
    .expect("writing succeeds");
    let _ = Frame::write(&mut writer, &spawn_request(served.project));

    let answer: ServerMessage = Frame::read(&mut reader).expect("refused out loud");
    assert!(matches!(
        answer,
        ServerMessage::Error {
            error: ProtocolError::IncompatibleVersion { .. }
        }
    ));
    assert!(hung_up(reader, PATIENCE), "the half it reads was left open");
    assert!(
        stops_listening(writer, PATIENCE),
        "the half it writes to was left open"
    );

    let seen = subscribe_and_collect(&served.endpoint, Duration::from_millis(500));
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. })),
        "the refused client's request was acted on: {seen:#?}"
    );
}

#[test]
fn a_client_that_never_says_hello_is_hung_up_on() {
    let served = served(
        "no-hello",
        Budgets {
            handshake: Duration::from_millis(200),
            ..Budgets::default()
        },
    );
    let (reader, _writer) = raw_client(&served.endpoint);

    assert!(
        hung_up(reader, PATIENCE),
        "a silent client is still connected"
    );
}

#[test]
fn a_client_that_stops_mid_frame_is_hung_up_on() {
    let served = served(
        "mid-frame",
        Budgets {
            frame: Duration::from_millis(200),
            ..Budgets::default()
        },
    );
    let (reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");

    // A length prefix promising a hundred bytes, then three of them.
    writer
        .write_all(&100u32.to_be_bytes())
        .and_then(|()| writer.write_all(b"abc"))
        .and_then(|()| writer.flush())
        .expect("writing succeeds");

    assert!(
        hung_up(reader, PATIENCE),
        "a client stalled part-way through a frame is still connected"
    );
}

#[test]
fn clients_past_the_limit_are_turned_away() {
    let served = served(
        "quota",
        Budgets {
            max_clients: 2,
            // Clear of PATIENCE, which equals the default: otherwise a
            // broken quota could be masked by the handshake deadline
            // hanging the third client up on its own, and the assertion
            // below would point at the wrong cause.
            handshake: Duration::from_secs(60),
            ..Budgets::default()
        },
    );

    let first = raw_client(&served.endpoint);
    let second = raw_client(&served.endpoint);
    // Both must be counted before the third arrives.
    std::thread::sleep(Duration::from_millis(200));

    let (third_reader, _third_writer) = raw_client(&served.endpoint);
    assert!(
        hung_up(third_reader, PATIENCE),
        "a third client was let in past a limit of two"
    );

    // The two already in are unaffected.
    for (mut reader, mut writer) in [first, second] {
        Frame::write(&mut writer, &hello()).expect("writing succeeds");
        let answer: ServerMessage = Frame::read(&mut reader).expect("still served");
        assert!(matches!(answer, ServerMessage::Welcome { .. }));
    }
}

/// Round 1, finding 1(a): a client that disconnects cleanly must free its
/// seat, not merely stop being able to use it.
#[test]
fn a_seat_freed_by_a_disconnected_client_admits_the_next_one() {
    let served = served(
        "seat-freed",
        Budgets {
            max_clients: 1,
            ..Budgets::default()
        },
    );

    {
        let (mut reader, mut writer) = raw_client(&served.endpoint);
        Frame::write(&mut writer, &hello()).expect("writing succeeds");
        let welcome: ServerMessage = Frame::read(&mut reader).expect("welcomed");
        assert!(matches!(welcome, ServerMessage::Welcome { .. }));
        // Both halves drop here, disconnecting.
    }

    // Retried rather than tried once: freeing the seat is asynchronous with
    // this end noticing the first client is gone, so the very next connect
    // attempt can still land before the daemon has caught up.
    let deadline = Instant::now() + PATIENCE;
    let mut admitted = false;
    while Instant::now() < deadline && !admitted {
        let (mut reader, mut writer) = raw_client(&served.endpoint);
        if Frame::write(&mut writer, &hello()).is_ok()
            && matches!(
                Frame::read::<_, ServerMessage>(&mut reader),
                Ok(ServerMessage::Welcome { .. })
            )
        {
            admitted = true;
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    assert!(
        admitted,
        "the seat freed by the first client was never reused"
    );
}

/// Round 1, finding 1(b): the audit's probe. A client whose reader has
/// ended -- an ordinary disconnect, from the daemon's point of view -- can
/// still be holding its seat open through a writer stuck delivering to it,
/// if freeing the seat is counted per thread rather than per connection.
#[test]
#[cfg(unix)]
fn a_seat_held_by_a_stuck_writer_is_freed_once_the_daemon_notices() {
    let served = served(
        "stuck-writer",
        Budgets {
            max_clients: 1,
            ..Budgets::default()
        },
    );

    let (reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");
    Frame::write(&mut writer, &ClientMessage::Subscribe).expect("writing succeeds");
    Frame::write(
        &mut writer,
        &ClientMessage::SpawnPane {
            project: served.project,
            harness: "flood".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    )
    .expect("writing succeeds");

    // Long enough for the pane to spawn and flood the socket with output
    // nothing here is draining, so the daemon's writer thread to this
    // client is genuinely stuck mid-write -- not merely queued -- by the
    // time the read half closes below. The default `outbox_bytes` (32 MiB)
    // is left in place so that budget cannot hang the client up on its own
    // first: what this test means to catch is the reader ending while the
    // writer is still blocked on the OS socket, not the daemon's own
    // live-traffic limit.
    std::thread::sleep(Duration::from_secs(1));

    // Shuts down only the half the daemon reads from -- an ordinary
    // disconnect to its reader thread -- while its writer, blocked as
    // above, is left mid-write. Never reading from `reader` is what keeps
    // it that way; it stays open, not dropped, until the loop below no
    // longer needs it.
    drop(writer);

    let deadline = Instant::now() + PATIENCE;
    let mut admitted = false;
    while Instant::now() < deadline && !admitted {
        let (mut second_reader, mut second_writer) = raw_client(&served.endpoint);
        if Frame::write(&mut second_writer, &hello()).is_ok()
            && matches!(
                Frame::read::<_, ServerMessage>(&mut second_reader),
                Ok(ServerMessage::Welcome { .. })
            )
        {
            admitted = true;
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    drop(reader);
    assert!(
        admitted,
        "a second client was never admitted past the limit of one"
    );
}

/// Round 1, finding 3: `enforce_deadlines` must not mistake an idle,
/// already-welcomed client for one that is late -- only a `Hello` not yet
/// said, or a frame started and not finished, is a deadline at all.
#[test]
fn a_ready_client_left_idle_past_its_budgets_is_still_served() {
    let served = served(
        "idle-ready",
        Budgets {
            frame: Duration::from_millis(200),
            handshake: Duration::from_millis(200),
            ..Budgets::default()
        },
    );

    let (mut reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");
    let welcome: ServerMessage = Frame::read(&mut reader).expect("welcomed");
    assert!(matches!(welcome, ServerMessage::Welcome { .. }));

    // Well past both budgets above, with nothing sent in between.
    std::thread::sleep(Duration::from_secs(1));

    Frame::write(&mut writer, &ClientMessage::Ping { token: 7 }).expect("writing succeeds");
    let answer: ServerMessage =
        Frame::read(&mut reader).expect("an idle, welcomed client is still served");
    assert!(matches!(answer, ServerMessage::Pong { token: 7 }));
}

/// Round 2, finding 1, assertion 2: pins the both-threads seat directly,
/// independent of `Daemon::hang_up`. A client refused mid-flood is only
/// forgotten by `Daemon::refuse`, which never closes its connection; its
/// writer, already genuinely stuck delivering to it, is left running all
/// the same, and its seat must stay held for exactly as long as that writer
/// does -- not released early just because the client's reader, separately,
/// has ended.
#[test]
#[cfg(unix)]
fn a_seat_held_by_a_refused_clients_stuck_writer_is_not_released_early() {
    let served = served(
        "refused-stuck",
        Budgets {
            max_clients: 1,
            ..Budgets::default()
        },
    );

    let (reader, mut writer) = raw_client(&served.endpoint);
    Frame::write(&mut writer, &hello()).expect("writing succeeds");
    Frame::write(&mut writer, &ClientMessage::Subscribe).expect("writing succeeds");
    Frame::write(
        &mut writer,
        &ClientMessage::SpawnPane {
            project: served.project,
            harness: "flood".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    )
    .expect("writing succeeds");

    // Never read; long enough for the writer to genuinely block on the
    // socket, same reasoning as the test above.
    std::thread::sleep(Duration::from_secs(1));

    // A second `Hello`, sent now that this client is already welcomed, is
    // refused for its version -- `Daemon::refuse`, not `Daemon::hang_up`:
    // it forgets the client but does not touch its connection, relying on
    // the writer thread to end on its own once nothing is left queued or a
    // write fails. A writer genuinely stuck mid-write does neither, which
    // is exactly the shape that would let a reader-only seat free itself
    // the moment the read half closes next, though nothing has actually
    // ended.
    Frame::write(
        &mut writer,
        &ClientMessage::Hello {
            version: dispatch_proto::Version {
                major: 99,
                minor: 0,
            },
            client: "future".into(),
            role: dispatch_proto::Role::Interface,
        },
    )
    .expect("writing succeeds");
    drop(writer);

    // Closing only the read half does not touch the OS-level write the
    // daemon's writer is blocked on, so the seat must still be held: no
    // second client is admitted for as long as this window runs.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut admitted = false;
    while Instant::now() < deadline && !admitted {
        let (mut second_reader, mut second_writer) = raw_client(&served.endpoint);
        if Frame::write(&mut second_writer, &hello()).is_ok()
            && matches!(
                Frame::read::<_, ServerMessage>(&mut second_reader),
                Ok(ServerMessage::Welcome { .. })
            )
        {
            admitted = true;
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(
        !admitted,
        "a second client was welcomed while the first's writer was still stuck"
    );

    // Only once the first client's other half closes too does its writer's
    // blocked write finally fail, freeing the seat for real.
    drop(reader);

    let deadline = Instant::now() + PATIENCE;
    let mut admitted = false;
    while Instant::now() < deadline && !admitted {
        let (mut second_reader, mut second_writer) = raw_client(&served.endpoint);
        if Frame::write(&mut second_writer, &hello()).is_ok()
            && matches!(
                Frame::read::<_, ServerMessage>(&mut second_reader),
                Ok(ServerMessage::Welcome { .. })
            )
        {
            admitted = true;
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(
        admitted,
        "a second client was never admitted once the first was fully gone"
    );
}

#[test]
fn shutting_down_ends_every_panes_whole_tree() {
    let (mut daemon, project, _dir) = daemon("tree-down");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "tree".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    wait_for(&mut daemon, &inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });

    let pid = daemon
        .pane_pids_for_test()
        .into_iter()
        .next()
        .expect("the pane has a pid");
    let deadline = Instant::now() + Duration::from_secs(10);
    let everyone = loop {
        let below = dispatch_os::process::descendants(pid);
        if !below.is_empty() {
            break std::iter::once(pid).chain(below).collect::<Vec<_>>();
        }
        assert!(
            Instant::now() < deadline,
            "the pane never started its children"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    daemon.shutdown_handle().request();
    daemon.run();

    let deadline = Instant::now() + Duration::from_secs(10);
    while everyone
        .iter()
        .any(|p| dispatch_os::process::is_running(*p))
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        everyone
            .iter()
            .all(|p| !dispatch_os::process::is_running(*p)),
        "a process a pane started outlived the daemon: {everyone:?}"
    );
}

/// Not a test of its own: what the `capture` harness runs inside a pane, to
/// record exactly what reached its standard input. Run without
/// `DISPATCH_CAPTURE_TO`, it does nothing.
#[test]
fn capture_standard_input() {
    let Some(out) = std::env::var_os("DISPATCH_CAPTURE_TO") else {
        return;
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("standard input is readable");
    std::fs::write(out, bytes).expect("the capture is writable");
}

/// Delegates `task` to a `capture` harness started as `command`, whose
/// `[task]` arguments `args` spells out given the binary that records what
/// it is sent, and checks that the task reached that binary's standard input
/// byte for byte, that nothing in it ran, and that its file went with the
/// run.
///
/// The agent is this test binary running [`capture_standard_input`], so what
/// it received can be compared with what was asked.
fn assert_a_task_reaches_the_capture_exactly(
    label: &str,
    command: &str,
    args: impl Fn(&Path) -> String,
    task: &str,
) {
    let dir = TempDir::new(label);
    let harness_dir = dir.0.join("harnesses");
    let _ = harnesses(&harness_dir);

    let captured = dir.0.join("captured.bin");
    let exe = std::env::current_exe().expect("the test binary");
    // A harness's own spelling of the task file's variable, naming another
    // file. On Windows it is the same variable, and the task's file has to
    // win it; elsewhere it is another variable, and nothing reads it.
    let decoy = dir.0.join("decoy.txt");
    std::fs::write(&decoy, "not the task").expect("temp dir is writable");
    std::fs::write(
        harness_dir.join("capture.toml"),
        format!(
            "id = \"capture\"\ndisplay_name = \"Capture\"\ncommand = \"{command}\"\n\n\
             [env]\nDISPATCH_CAPTURE_TO = '{}'\nDispatch_Task_File = '{}'\n\n\
             [task]\nargs = {}\ninput = \"file\"\n",
            captured.display(),
            decoy.display(),
            args(&exe)
        ),
    )
    .expect("temp dir is writable");

    let registry = HarnessRegistry::load_from_dir(&harness_dir).expect("loading succeeds");
    let mut daemon = Daemon::new(registry, "test-device");
    // Every character cmd.exe would act on outside quotes -- a space, as in
    // many Windows profile paths, `&`, `(`, `)`, `^`, a `%VAR%` and a
    // `!VAR!` -- so the file's path is shown to reach cmd.exe's `<` whole,
    // quoted and expanded once.
    let task_dir = dir.0.join("task files & (x) ^ %PATH% !y!");
    daemon.set_task_dir(task_dir.clone());
    let project =
        daemon.open_project(dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves"));

    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "capture".into(),
            task: task.into(),
            size: (80, 24),
            handoff: Some(handoff_for(task)),
            interactive: false,
        },
    );
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    assert!(
        seen.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { exit: 0, .. })),
        "the capture ran and succeeded: {seen:#?}"
    );

    // The agent is handed the brief, which carries the handoff verbatim after
    // its preamble, so the task arrives intact exactly when that text does.
    let received = std::fs::read(&captured).expect("the agent recorded its input");
    let handoff = handoff_for(task).text.into_bytes();
    assert!(
        received.ends_with(&handoff),
        "the task arrived changed: {}",
        String::from_utf8_lossy(&received)
    );
    assert!(
        !dir.0.join("marker.txt").exists(),
        "a command inside the task ran"
    );
    assert!(
        std::fs::read_dir(&task_dir).map_or(true, |mut d| d.next().is_none()),
        "the task's file outlived the run"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "exercises cmd.exe")]
fn a_task_reaches_a_cmd_wrapped_agent_exactly() {
    // The audit's A01: through `cmd.exe /c agent {task}`, `&` in a task ran
    // a second command. Here the agent is this test binary, reached through
    // cmd.exe exactly as claude.cmd is, recording what its standard input
    // received.
    assert_a_task_reaches_the_capture_exactly(
        "cmd-task",
        "cmd.exe",
        |exe| {
            format!(
                "[\"/d\", \"/v:off\", \"/c\", '{}', \"--exact\", \
                 \"session::tests::capture_standard_input\", \"--nocapture\", \
                 \"<%DISPATCH_TASK_FILE%\"]",
                exe.display()
            )
        },
        "literal & echo DISPATCH_AUDIT_MARKER> marker.txt | \"quoted\" %PATH% !PATH! ^caret\r\n\
         second line \u{fc}n\u{ef}c\u{f8}d\u{e9} \u{2713}",
    );
}

#[test]
#[cfg(unix)]
fn a_task_redirected_by_a_posix_shell_reaches_the_agent_exactly() {
    // The same delivery where it can run on every machine: the daemon writes
    // the task down, names the file in the environment, and removes it once
    // the run is over. Here `sh` does the redirecting, quoting the variable
    // itself.
    assert_a_task_reaches_the_capture_exactly(
        "sh-task",
        "sh",
        |exe| {
            format!(
                "[\"-c\", 'exec \"$0\" --exact session::tests::capture_standard_input \
                 --nocapture < \"$DISPATCH_TASK_FILE\"', '{}']",
                exe.display()
            )
        },
        "literal; echo DISPATCH_AUDIT_MARKER > marker.txt $(touch marker.txt) `touch marker.txt` \
         \"quoted\" '$HOME'\r\nsecond line \u{fc}n\u{ef}c\u{f8}d\u{e9} \u{2713}",
    );
}

/// A daemon serving the fixture harnesses plus `extra`, with a parent pane
/// and a delegate caller attached, and the request for `task` under
/// `harness` already made.
///
/// Returns the daemon, the interface client, the caller, and the test's
/// directory, which the daemon's project and task files live in.
fn delegating_to(
    label: &str,
    extra: &[(&str, String)],
    harness: &str,
    task: &str,
) -> (Daemon, Inbox, Inbox, TempDir) {
    let dir = TempDir::new(label);
    let harness_dir = dir.0.join("harnesses");
    let _ = harnesses(&harness_dir);
    for (name, body) in extra {
        std::fs::write(harness_dir.join(format!("{name}.toml")), body)
            .expect("temp dir is writable");
    }

    let registry = HarnessRegistry::load_from_dir(&harness_dir).expect("loading succeeds");
    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    let project =
        daemon.open_project(dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves"));
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: harness.into(),
            task: task.into(),
            size: (80, 24),
            handoff: Some(handoff_for(task)),
            interactive: false,
        },
    );

    (daemon, ui, caller, dir)
}

/// A harness whose command is the bare name `agent`, looked for only in
/// `bin`, where `.CMD` completes it: what `claude` written bare is on a
/// machine where npm installed `claude.cmd`.
fn bare_agent(bin: &Path) -> String {
    format!(
        "id = \"agent\"\ndisplay_name = \"Agent\"\ncommand = \"agent\"\n\n\
         [env]\nPATH = '{}'\nPATHEXT = \".CMD\"\n\n\
         [task]\nargs = [\"{{task}}\"]\n",
        bin.display()
    )
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "a batch file runs through cmd.exe only on Windows"
)]
fn a_command_windows_finds_as_a_batch_file_is_refused_before_anyone_is_asked() {
    let bin = TempDir::new("batch-bin");
    std::fs::write(bin.0.join("agent.CMD"), "@echo ran\r\n").expect("temp dir is writable");

    let (daemon, ui, caller, _dir) = delegating_to(
        "batch-on-path",
        &[("agent", bare_agent(&bin.0))],
        "agent",
        "x & echo DISPATCH_AUDIT_MARKER",
    );

    let told = outcomes(&drain(&caller));
    assert!(
        told.iter().any(|o| matches!(
            o,
            DelegateOutcome::Refused { reason }
                if reason.contains("agent.toml") && reason.to_lowercase().contains("agent.cmd")
        )),
        "the caller is told what was found and which file to fix: {told:?}"
    );
    assert!(
        pending(&drain(&ui)).is_none(),
        "nobody is asked to approve it"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing started");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "a batch file runs through cmd.exe only on Windows"
)]
fn a_command_that_becomes_a_batch_file_while_asking_is_refused_at_approval() {
    // Judged when the request arrived, `agent` found nothing. By the time
    // the user approves, `agent.CMD` is there -- and what starts is decided
    // when it starts.
    let bin = TempDir::new("late-bin");
    let (mut daemon, ui, caller, dir) = delegating_to(
        "late-batch",
        &[("agent", bare_agent(&bin.0))],
        "agent",
        // No space, so no quotes around it on the command line: the shape
        // that ran a second command.
        "x&echo.DISPATCH_AUDIT_MARKER>marker.txt",
    );
    let request = pending(&drain(&ui)).expect("nothing is a batch file yet, so the user is asked");

    std::fs::write(bin.0.join("agent.CMD"), "@echo ran\r\n").expect("temp dir is writable");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let told = outcomes(&drain(&caller));
    assert!(
        told.iter().any(|o| matches!(
            o,
            DelegateOutcome::Refused { reason } if reason.to_lowercase().contains("agent.cmd")
        )),
        "the approval is judged again on what would start: {told:?}"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing started");
    // Given time to have run, had it started.
    for _ in 0..50 {
        daemon.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !dir.0.join("marker.txt").exists(),
        "a command inside the task ran"
    );
}

/// A daemon whose delegated pane was closed while its task's file could not
/// be removed, as on Windows while a killed agent still holds it open: here
/// the directory is made read-only for the close.
///
/// Returns the daemon, the task directory (read-only again only if the
/// caller makes it so), the file left behind, and the test's directory. `None`
/// when permissions stop nobody, as for root.
#[cfg(unix)]
fn with_a_task_file_the_close_left(label: &str) -> Option<(Daemon, PathBuf, PathBuf, TempDir)> {
    use std::os::unix::fs::PermissionsExt;

    let hold = "id = \"hold\"\ndisplay_name = \"Hold\"\ncommand = \"sh\"\n\n\
                [task]\nargs = [\"-c\", 'exec sleep 30 < \"$DISPATCH_TASK_FILE\"']\n\
                input = \"file\"\n";
    let (mut daemon, ui, caller, dir) =
        delegating_to(label, &[("hold", hold.to_string())], "hold", "a task");
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    let pane = outcomes(&drain(&caller))
        .into_iter()
        .find_map(|o| match o {
            DelegateOutcome::Approved { pane } => Some(pane),
            _ => None,
        })
        .expect("the subagent started");

    let tasks = dir.0.join("tasks");
    let file = std::fs::read_dir(&tasks)
        .expect("the task directory exists")
        .flatten()
        .map(|entry| entry.path())
        .next()
        .expect("the task was written down");

    let set = |mode| {
        std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(mode))
            .expect("permissions change");
    };
    set(0o500);
    if std::fs::write(tasks.join("probe"), "").is_ok() {
        set(0o700);
        eprintln!("skipped: permissions do not stop this user writing");
        return None;
    }

    daemon.request_for_test(1, ClientMessage::ClosePane { pane });
    assert!(file.exists(), "the close could remove it after all");
    set(0o700);

    Some((daemon, tasks, file, dir))
}

#[test]
#[cfg(unix)]
fn a_task_file_a_closed_pane_left_is_removed_on_a_later_pass() {
    let Some((mut daemon, _tasks, file, _dir)) = with_a_task_file_the_close_left("retry-pass")
    else {
        return;
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    while file.exists() && Instant::now() < deadline {
        daemon.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!file.exists(), "the task's file is still there");
}

#[test]
#[cfg(unix)]
fn a_task_file_a_closed_pane_left_is_removed_at_shutdown() {
    let Some((mut daemon, _tasks, file, _dir)) = with_a_task_file_the_close_left("retry-shutdown")
    else {
        return;
    };

    // Asked to stop before the loop takes a single pass: only the way out
    // is left to try again.
    daemon.shutdown_handle().request();
    daemon.run();

    assert!(!file.exists(), "the task's file outlived the daemon");
}

#[test]
fn a_daemon_that_starts_serving_clears_away_task_files_left_behind() {
    // A daemon that was killed never dropped its panes, so their task files
    // stayed. The next to serve this configuration is the only one that
    // could clear them, and nothing still means to hand them over. Only
    // names Dispatch gives are touched.
    let (daemon, _project, dir) = daemon("sweep");
    let tasks = dir.0.join("tasks");
    // Made as the daemon makes it. Made any other way on Windows under an
    // elevated account -- as CI runs -- the Administrators group would own
    // it, and the daemon rightly refuses a directory this user does not.
    dispatch_os::paths::create_private_dir(&tasks).expect("temp dir is writable");
    let left = tasks.join(format!(
        "dispatch-task-{}.txt",
        dispatch_core::RequestId::new()
    ));
    let lookalike = tasks.join("dispatch-task-notes.txt");
    let unrelated = tasks.join("notes.txt");
    for path in [&left, &lookalike, &unrelated] {
        std::fs::write(path, "a task").expect("temp dir is writable");
    }

    let listener = Listener::bind_to(&dir.0.join("d.sock")).expect("binding succeeds");
    let shutdown = daemon.shutdown_handle();
    let serving = std::thread::spawn(move || {
        let _ = daemon.serve(listener);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while left.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    shutdown.request();
    serving.join().expect("the daemon stops");

    assert!(!left.exists(), "a task file left behind is still there");
    assert!(
        lookalike.exists() && unrelated.exists(),
        "a file Dispatch did not name was removed"
    );
}

#[test]
#[cfg(unix)]
fn the_sweep_never_reads_through_a_linked_directory() {
    // What a link names is somebody's choice, not Dispatch's directory.
    let dir = TempDir::new("sweep-link");
    let elsewhere = dir.0.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("temp dir is writable");
    let left = elsewhere.join(format!(
        "dispatch-task-{}.txt",
        dispatch_core::RequestId::new()
    ));
    std::fs::write(&left, "a task").expect("temp dir is writable");
    let tasks = dir.0.join("tasks");
    std::os::unix::fs::symlink(&elsewhere, &tasks).expect("the file system links");

    crate::task_file::sweep(&tasks);

    assert!(left.exists(), "the sweep reached through the link");
}

#[test]
fn only_the_daemon_holding_the_task_directory_sweeps_it() {
    // Two daemons can share a task directory -- on Linux, different
    // XDG_CONFIG_HOMEs and one XDG_DATA_HOME -- and bind different endpoints,
    // so binding proves nothing about the directory. Its lock does.
    let dir = TempDir::new("sweep-lock");
    let tasks = dir.0.join("tasks");
    // Made as the daemon makes it. Made any other way on Windows under an
    // elevated account -- as CI runs -- the Administrators group would own
    // it, and the daemon rightly refuses a directory this user does not.
    dispatch_os::paths::create_private_dir(&tasks).expect("temp dir is writable");
    let left = tasks.join(format!(
        "dispatch-task-{}.txt",
        dispatch_core::RequestId::new()
    ));
    std::fs::write(&left, "another daemon's live task").expect("temp dir is writable");

    let other = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(tasks.join(crate::task_file::LOCK_FILE))
        .expect("the lock file opens");
    other.try_lock().expect("nobody holds it yet");

    assert!(
        crate::task_file::claim(&tasks).is_none(),
        "the directory was claimed while another daemon held it"
    );
    assert!(left.exists(), "another daemon's task file was swept");

    drop(other);
    let held = crate::task_file::claim(&tasks);
    assert!(held.is_some(), "a free directory was not claimed");
    assert!(!left.exists(), "the holder did not sweep");
}

#[test]
fn a_harness_spelling_of_a_daemon_variable_is_the_one_its_agent_gets() {
    // On Windows `Dispatch_Pane` and `DISPATCH_PANE` are one variable, so
    // the harness's spelling replaces the daemon's rather than sitting
    // beside it for the spawn to choose between. Elsewhere they are two.
    let spelled = "id = \"spelled\"\ndisplay_name = \"Spelled\"\ncommand = \"agent\"\n\n\
                   [env]\nDispatch_Pane = \"the harness's\"\n\n\
                   [task]\nargs = [\"{task}\"]\n";
    let (daemon, _ui, _caller, _dir) = delegating_to(
        "case-variant-env",
        &[("spelled", spelled.to_string())],
        "spelled",
        "anything",
    );

    let run = daemon
        .task_run("spelled", "anything", PaneId::new(), false)
        .expect("the harness has a task form");
    let spellings: Vec<_> = run
        .launch
        .env
        .keys()
        .filter(|name| name.eq_ignore_ascii_case("DISPATCH_PANE"))
        .map(String::as_str)
        .collect();

    if cfg!(windows) {
        assert_eq!(spellings, ["Dispatch_Pane"], "one variable, the harness's");
    } else {
        assert_eq!(
            spellings,
            ["DISPATCH_PANE", "Dispatch_Pane"],
            "two variables"
        );
    }
    assert_eq!(
        run.launch.env.get("Dispatch_Pane").map(String::as_str),
        Some("the harness's")
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "a batch file runs through cmd.exe only on Windows"
)]
fn a_batch_file_on_a_path_spelled_as_windows_spells_it_is_refused() {
    // `Path` is how Windows itself spells it, and so how a harness written
    // there does.
    let bin = TempDir::new("spelled-bin");
    std::fs::write(bin.0.join("agent.CMD"), "@echo ran\r\n").expect("temp dir is writable");
    let agent = bare_agent(&bin.0).replace("[env]\nPATH = ", "[env]\nPath = ");
    assert!(agent.contains("Path = "), "the fixture spells it Path");

    let (daemon, ui, caller, _dir) =
        delegating_to("spelled-path", &[("agent", agent)], "agent", "anything");

    let told = outcomes(&drain(&caller));
    assert!(
        told.iter().any(|o| matches!(
            o,
            DelegateOutcome::Refused { reason } if reason.to_lowercase().contains("agent.cmd")
        )),
        "the batch file on Path is refused: {told:?}"
    );
    assert!(
        pending(&drain(&ui)).is_none(),
        "nobody is asked to approve it"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing started");
}

#[test]
fn an_argument_form_is_never_handed_a_task_file() {
    // Its task is in its arguments. A DISPATCH_TASK_FILE in its environment
    // could only be stale -- inherited, or set in the harness file -- and a
    // redirect written against it would read some other file.
    let argue = "id = \"argue\"\ndisplay_name = \"Argue\"\ncommand = \"agent\"\n\n\
                 [env]\nDISPATCH_TASK_FILE = \"elsewhere.txt\"\n\n\
                 [task]\nargs = [\"{task}\"]\n";
    let (daemon, _ui, _caller, _dir) = delegating_to(
        "argument-form-env",
        &[("argue", argue.to_string())],
        "argue",
        "anything",
    );

    let run = daemon
        .task_run("argue", "anything", PaneId::new(), false)
        .expect("the harness has a task form");
    assert_eq!(run.input, TaskInput::Argument);
    assert!(
        !run.launch.env.contains_key(dispatch_config::TASK_FILE_ENV),
        "the harness file's value is passed on: {:?}",
        run.launch.env
    );
    assert!(
        run.launch.unset.contains(dispatch_config::TASK_FILE_ENV),
        "a value this process holds would be inherited"
    );
}

#[test]
fn a_file_form_that_also_names_the_task_is_refused_before_anyone_is_asked() {
    // On every platform: a file form fills nothing in, so its `{task}` would
    // reach the agent as those six characters.
    let dir = TempDir::new("mixed-form");
    let harness_dir = dir.0.join("harnesses");
    let _ = harnesses(&harness_dir);
    std::fs::write(
        harness_dir.join("mixed.toml"),
        "id = \"mixed\"\ndisplay_name = \"Mixed\"\ncommand = \"agent\"\n\n\
         [task]\nargs = [\"-p\", \"{task}\"]\ninput = \"file\"\n",
    )
    .expect("temp dir is writable");

    let registry = HarnessRegistry::load_from_dir(&harness_dir).expect("loading succeeds");
    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    let project =
        daemon.open_project(dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves"));
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "mixed".into(),
            task: "anything".into(),
            size: (80, 24),
            handoff: Some(handoff_for("anything")),
            interactive: false,
        },
    );

    let told = outcomes(&drain(&caller));
    assert!(
        told.iter().any(|o| matches!(
            o,
            DelegateOutcome::Refused { reason } if reason.contains("mixed.toml") && reason.contains("{task}")
        )),
        "the caller is told which file to fix: {told:?}"
    );
    assert!(
        pending(&drain(&ui)).is_none(),
        "nobody is asked to approve it"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing started");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the refusal is for cmd.exe, which only Windows has"
)]
fn a_task_form_that_would_put_the_task_on_cmds_command_line_is_refused() {
    // What every Windows installation from before this fix still has in any
    // harness file its user edited.
    let dir = TempDir::new("unsafe-form");
    let harness_dir = dir.0.join("harnesses");
    let _ = harnesses(&harness_dir);
    std::fs::write(
        harness_dir.join("old.toml"),
        "id = \"old\"\ndisplay_name = \"Old\"\ncommand = \"cmd.exe\"\n\n[task]\nargs = [\"/c\", \"{task}\"]\n",
    )
    .expect("temp dir is writable");

    let registry = HarnessRegistry::load_from_dir(&harness_dir).expect("loading succeeds");
    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    let project =
        daemon.open_project(dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves"));
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "old".into(),
            task: "x & echo DISPATCH_AUDIT_MARKER".into(),
            size: (80, 24),
            handoff: Some(handoff_for("x & echo DISPATCH_AUDIT_MARKER")),
            interactive: false,
        },
    );

    let told = outcomes(&drain(&caller));
    assert!(
        told.iter().any(|o| matches!(
            o,
            DelegateOutcome::Refused { reason } if reason.contains("old.toml") && reason.contains("cmd.exe")
        )),
        "the caller is told which file to fix: {told:?}"
    );
    assert!(
        pending(&drain(&ui)).is_none(),
        "nobody is asked to approve it"
    );
    assert_eq!(daemon.pane_count(), 1, "nothing started");
}

/// Makes `dir` a repository on `branch`, as far as reading `HEAD` goes.
fn check_out(dir: &Path, branch: &str) {
    std::fs::create_dir_all(dir.join(".git")).expect("temp dir is writable");
    std::fs::write(
        dir.join(".git").join("HEAD"),
        format!("ref: refs/heads/{branch}\n"),
    )
    .expect("temp dir is writable");
}

/// A daemon with one project in a repository on `main`, looking at branches
/// on every tick rather than every two seconds.
fn daemon_in_a_repository(label: &str) -> (Daemon, ProjectId, TempDir) {
    let dir = TempDir::new(label);
    check_out(&dir.0, "main");
    let registry = harnesses(&dir.0.join("harnesses"));

    let mut daemon = Daemon::new(registry, "test-device");
    daemon.branch_every = Duration::ZERO;
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");
    let project = daemon.open_project(root);

    (daemon, project, dir)
}

/// The pane a `PaneSpawned` among `messages` announced.
///
/// Used only by the tests that read a pane's working directory, which no
/// Windows build can.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawned_pane(messages: &[ServerMessage]) -> Option<PaneId> {
    messages.iter().find_map(|message| match message {
        ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
        _ => None,
    })
}

/// The branch last reported for `pane`, if any report was seen.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn last_branch(messages: &[ServerMessage], pane: PaneId) -> Option<Option<String>> {
    messages.iter().rev().find_map(|message| match message {
        ServerMessage::PaneChanged {
            pane: changed,
            update: PaneUpdate::Branch { branch },
        } if *changed == pane => Some(branch.clone()),
        _ => None,
    })
}

#[test]
fn a_project_in_a_repository_is_announced_with_its_branch() {
    let (mut daemon, project, _dir) = daemon_in_a_repository("branch-open");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);

    let announced = drain(&inbox)
        .into_iter()
        .find_map(|message| match message {
            ServerMessage::ProjectOpened { project: opened } if opened.id == project => {
                Some(opened)
            }
            _ => None,
        })
        .expect("the project is announced on subscribe");

    assert_eq!(announced.branch.as_deref(), Some("main"));
}

#[test]
fn switching_a_projects_branch_is_announced() {
    let (mut daemon, project, dir) = daemon_in_a_repository("branch-switch");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    check_out(&dir.0, "feat/tabs");

    wait_for(&mut daemon, &inbox, |messages| {
        messages.iter().any(|message| {
            matches!(
                message,
                ServerMessage::ProjectChanged {
                    project: changed,
                    update: ProjectUpdate::Branch { branch: Some(branch) },
                } if *changed == project && branch == "feat/tabs"
            )
        })
    });
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_pane_reports_the_branch_of_the_directory_its_shell_is_in() {
    let (mut daemon, project, dir) = daemon_in_a_repository("branch-pane");
    check_out(&dir.0.join("wt"), "feat/wt");

    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |messages| {
        spawned_pane(messages).and_then(|pane| last_branch(messages, pane))
            == Some(Some("main".into()))
    });
    let pane = spawned_pane(&seen).expect("the pane was announced");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"cd wt\n".to_vec(),
        },
    );
    wait_for(&mut daemon, &inbox, |messages| {
        last_branch(messages, pane) == Some(Some("feat/wt".into()))
    });

    // A client attaching now is told where the pane is, rather than left to
    // wait for it to move again.
    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    assert_eq!(
        last_branch(&drain(&late), pane),
        Some(Some("feat/wt".into()))
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn an_exited_pane_keeps_the_branch_it_last_had() {
    // Between its process exiting and the exit being noticed, a pane's
    // directory cannot be read. Reporting that as "no branch" would bounce
    // the finished pane out of its group for no reason.
    let (mut daemon, project, _dir) = daemon_in_a_repository("branch-exit");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&inbox);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    let seen = wait_for(&mut daemon, &inbox, |messages| {
        spawned_pane(messages).and_then(|pane| last_branch(messages, pane))
            == Some(Some("main".into()))
    });
    let pane = spawned_pane(&seen).expect("the pane was announced");
    // Kept rather than shadowed: broadcasting only ever happens on a change,
    // so this pane's one and only `Branch` report is the one already seen
    // above, and "the last word" has to be read across the whole history or
    // there will be no word here at all.
    let mut all = seen;

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"exit\n".to_vec(),
        },
    );
    all.extend(wait_for(&mut daemon, &inbox, |messages| {
        messages.iter().any(|message| {
            matches!(
                message,
                ServerMessage::PaneChanged {
                    pane: changed,
                    update: PaneUpdate::Status { status: PaneStatus::Exited(_) },
                } if *changed == pane
            )
        })
    }));

    for _ in 0..20 {
        daemon.tick();
        all.extend(drain(&inbox));
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        last_branch(&all, pane),
        Some(Some("main".into())),
        "the last word on the pane's branch is still main: {all:#?}"
    );
}

/// Attaches interface client `id`, subscribes it, and clears its inbox.
fn subscribed(daemon: &mut Daemon, id: u64) -> Inbox {
    let inbox = daemon.attach_for_test(id);
    daemon.request_for_test(id, hello());
    daemon.request_for_test(id, ClientMessage::Subscribe);
    let _ = drain(&inbox);
    inbox
}

/// Each tab's panes from the last `Tabs` seen for `project`.
fn last_tabs(messages: &[ServerMessage], project: ProjectId) -> Option<Vec<Vec<PaneId>>> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Tabs { project: p, tabs } if *p == project => {
            Some(tabs.iter().map(|tab| tab.panes.clone()).collect())
        }
        _ => None,
    })
}

/// The id of tab `index` in the last `Tabs` seen for `project`.
fn tab_at(messages: &[ServerMessage], project: ProjectId, index: usize) -> TabId {
    messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::Tabs { project: p, tabs } if *p == project => {
                tabs.get(index).map(|tab| tab.id)
            }
            _ => None,
        })
        .expect("the tab is in the last snapshot")
}

/// Spawns a pane with `place`, as client 1, and waits for the snapshot that
/// places it. Returns the pane and everything seen on the way.
fn spawn_placed(
    daemon: &mut Daemon,
    inbox: &Inbox,
    project: ProjectId,
    place: Placement,
) -> (PaneId, Vec<ServerMessage>) {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place,
            settings: Default::default(),
        },
    );

    let seen = wait_for(daemon, inbox, |m| {
        m.iter().any(|m| matches!(m, ServerMessage::Tabs { .. }))
    });
    let pane = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned");
    (pane, seen)
}

/// Whether a client was refused with exactly `reason`.
fn refused_with(messages: &[ServerMessage], reason: &str) -> bool {
    messages.iter().any(|m| {
        matches!(
            m,
            ServerMessage::Error { error: ProtocolError::Other(text) } if text == reason
        )
    })
}

#[test]
fn a_spawned_pane_is_placed_and_everyone_is_told_after_the_announcement() {
    let (mut daemon, project, _dir) = daemon("tabs-spawn");
    let inbox = subscribed(&mut daemon, 1);

    let (pane, seen) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);

    assert_eq!(last_tabs(&seen, project), Some(vec![vec![pane]]));
    let announced = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
        .expect("the pane was announced");
    let placed = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::Tabs { .. }))
        .expect("the pane was placed");
    assert!(
        announced < placed,
        "a snapshot never names a pane not yet announced"
    );
}

#[test]
fn a_spawn_asked_into_a_full_tab_opens_the_next_one() {
    let (mut daemon, project, _dir) = daemon("tabs-full-spawn");
    let inbox = subscribed(&mut daemon, 1);
    let mut all = Vec::new();
    let mut seen = Vec::new();
    for _ in 0..4 {
        let (pane, placed) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
        all.push(pane);
        seen = placed;
    }
    let first = tab_at(&seen, project, 0);

    let (fifth, seen) = spawn_placed(&mut daemon, &inbox, project, Placement::Into { tab: first });

    assert_eq!(
        last_tabs(&seen, project),
        Some(vec![all, vec![fifth]]),
        "the full tab is untouched and the new one follows it"
    );
}

#[test]
fn a_client_attaching_later_hears_every_projects_tabs_after_their_panes() {
    let (mut daemon, project, dir) = daemon("tabs-replay");
    let inbox = subscribed(&mut daemon, 1);
    let (a, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let (b, _) = spawn_placed(
        &mut daemon,
        &inbox,
        project,
        Placement::NewAfter { tab: None },
    );
    let empty_root = dir.0.join("empty");
    std::fs::create_dir_all(&empty_root).expect("temp dir is writable");
    let empty = daemon
        .open_project(dispatch_os::paths::resolve(&empty_root).expect("the temp dir resolves"));

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    let replay = drain(&late);

    assert_eq!(last_tabs(&replay, project), Some(vec![vec![a], vec![b]]));
    assert_eq!(
        last_tabs(&replay, empty),
        Some(Vec::new()),
        "an empty project still says it keeps tabs"
    );
    let last_pane = replay
        .iter()
        .rposition(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
        .expect("the panes were replayed");
    let first_tabs = replay
        .iter()
        .position(|m| matches!(m, ServerMessage::Tabs { .. }))
        .expect("the tabs were replayed");
    assert!(
        last_pane < first_tabs,
        "tabs come after every pane they name"
    );
}

#[test]
fn a_project_opened_while_running_says_it_keeps_tabs() {
    // A client reads a project with no tabs as one on a daemon too old for
    // them, so a project opened after the subscribe replay has to say so too.
    let (mut daemon, _, dir) = daemon("tabs-open");
    let inbox = subscribed(&mut daemon, 1);
    let fresh = dir.0.join("fresh");
    std::fs::create_dir_all(&fresh).expect("temp dir is writable");

    daemon.request_for_test(1, ClientMessage::OpenProject { root: fresh });

    let seen = drain(&inbox);
    let (opened, project) = seen
        .iter()
        .enumerate()
        .find_map(|(at, m)| match m {
            ServerMessage::ProjectOpened { project } => Some((at, project.id)),
            _ => None,
        })
        .expect("the project was announced");
    let tabs = seen
        .iter()
        .position(|m| {
            matches!(m, ServerMessage::Tabs { project: p, tabs } if *p == project && tabs.is_empty())
        })
        .unwrap_or_else(|| panic!("the project's empty tabs were sent: {seen:#?}"));
    assert!(opened < tabs, "the tabs follow the project they belong to");
}

#[test]
fn moving_a_pane_tells_everyone() {
    let (mut daemon, project, _dir) = daemon("tabs-move");
    let inbox = subscribed(&mut daemon, 1);
    let (a, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let (b, seen) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let first = tab_at(&seen, project, 0);

    daemon.request_for_test(
        1,
        ClientMessage::MovePane {
            pane: b,
            to: Placement::NewAfter { tab: Some(first) },
        },
    );

    assert_eq!(
        last_tabs(&drain(&inbox), project),
        Some(vec![vec![a], vec![b]])
    );
}

#[test]
fn the_second_of_two_moves_into_the_last_slot_is_refused() {
    // Two clients can each see room for one more pane. The daemon decides,
    // so only one of them gets it.
    let (mut daemon, project, _dir) = daemon("tabs-race");
    let first_client = subscribed(&mut daemon, 1);
    let second_client = subscribed(&mut daemon, 2);
    let mut seen = spawn_placed(&mut daemon, &first_client, project, Placement::Auto).1;
    for _ in 0..2 {
        seen = spawn_placed(&mut daemon, &first_client, project, Placement::Auto).1;
    }
    let first = tab_at(&seen, project, 0);
    let (d, _) = spawn_placed(
        &mut daemon,
        &first_client,
        project,
        Placement::NewAfter { tab: None },
    );
    let (e, _) = spawn_placed(
        &mut daemon,
        &first_client,
        project,
        Placement::NewAfter { tab: None },
    );
    let _ = drain(&second_client);

    daemon.request_for_test(
        1,
        ClientMessage::MovePane {
            pane: d,
            to: Placement::Into { tab: first },
        },
    );
    daemon.request_for_test(
        2,
        ClientMessage::MovePane {
            pane: e,
            to: Placement::Into { tab: first },
        },
    );

    assert!(refused_with(
        &drain(&second_client),
        "that tab is full (4 panes)"
    ));
    let tabs = last_tabs(&drain(&first_client), project).expect("the first move was told");
    assert_eq!(tabs[0].len(), 4);
    assert_eq!(tabs[0][3], d, "the first to ask got the slot");
    assert_eq!(tabs[1], vec![e]);
}

#[test]
fn a_subagent_is_never_placed_or_moved() {
    let (mut daemon, project, _dir) = daemon("tabs-subagent");
    let ui = subscribed(&mut daemon, 1);
    let (parent, _) = spawn_placed(&mut daemon, &ui, project, Placement::Auto);
    let _caller = ask(&mut daemon, parent, "echo delegated");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    let seen = wait_for(&mut daemon, &ui, |m| m.iter().any(m_is_child));
    let child = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned {
                pane,
                parent: Some(_),
                ..
            } => Some(*pane),
            _ => None,
        })
        .expect("the subagent was announced");

    assert!(
        !seen.iter().any(|m| matches!(
            m,
            ServerMessage::Tabs { tabs, .. } if tabs.iter().any(|tab| tab.panes.contains(&child))
        )),
        "no snapshot places the subagent"
    );

    daemon.request_for_test(
        1,
        ClientMessage::MovePane {
            pane: child,
            to: Placement::NewAfter { tab: None },
        },
    );
    assert!(refused_with(
        &drain(&ui),
        "a subagent stays beside the pane that asked for it"
    ));
}

#[test]
fn renaming_a_tab_tells_everyone() {
    let (mut daemon, project, _dir) = daemon("tabs-rename");
    let inbox = subscribed(&mut daemon, 1);
    let (_, seen) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let tab = tab_at(&seen, project, 0);

    daemon.request_for_test(
        1,
        ClientMessage::RenameTab {
            tab,
            name: "  work  ".into(),
        },
    );

    let named = drain(&inbox).into_iter().rev().find_map(|m| match m {
        ServerMessage::Tabs { tabs, .. } => tabs.first().and_then(|tab| tab.name.clone()),
        _ => None,
    });
    assert_eq!(named.as_deref(), Some("work"));
}

#[test]
fn a_tab_that_is_gone_is_reported() {
    let (mut daemon, _project, _dir) = daemon("tabs-gone");
    let inbox = subscribed(&mut daemon, 1);

    daemon.request_for_test(
        1,
        ClientMessage::RenameTab {
            tab: TabId::new(),
            name: "work".into(),
        },
    );
    daemon.request_for_test(
        1,
        ClientMessage::MoveTab {
            tab: TabId::new(),
            index: 0,
        },
    );
    daemon.request_for_test(1, ClientMessage::CloseTab { tab: TabId::new() });

    let refusals = drain(&inbox)
        .iter()
        .filter(|m| {
            matches!(
                m,
                ServerMessage::Error { error: ProtocolError::Other(text) } if text == "that tab is gone"
            )
        })
        .count();
    assert_eq!(refusals, 3);
}

#[test]
fn closing_a_tab_closes_exactly_its_panes() {
    let (mut daemon, project, _dir) = daemon("tabs-close");
    let inbox = subscribed(&mut daemon, 1);
    let (a, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let (b, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let (c, seen) = spawn_placed(
        &mut daemon,
        &inbox,
        project,
        Placement::NewAfter { tab: None },
    );
    let first = tab_at(&seen, project, 0);

    daemon.request_for_test(1, ClientMessage::CloseTab { tab: first });

    let seen = drain(&inbox);
    for pane in [a, b] {
        assert!(
            seen.iter()
                .any(|m| matches!(m, ServerMessage::PaneClosed { pane: p } if *p == pane)),
            "{pane} was closed"
        );
    }
    assert_eq!(last_tabs(&seen, project), Some(vec![vec![c]]));
    assert_eq!(daemon.pane_count(), 1);
}

#[test]
fn a_tab_moves_along_the_row() {
    let (mut daemon, project, _dir) = daemon("tabs-reorder");
    let inbox = subscribed(&mut daemon, 1);
    let (a, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);
    let (b, _) = spawn_placed(
        &mut daemon,
        &inbox,
        project,
        Placement::NewAfter { tab: None },
    );
    let (c, seen) = spawn_placed(
        &mut daemon,
        &inbox,
        project,
        Placement::NewAfter { tab: None },
    );
    let first = tab_at(&seen, project, 0);

    daemon.request_for_test(
        1,
        ClientMessage::MoveTab {
            tab: first,
            index: 2,
        },
    );

    assert_eq!(
        last_tabs(&drain(&inbox), project),
        Some(vec![vec![b], vec![c], vec![a]])
    );
}

#[test]
fn a_pane_that_exits_leaves_its_tab() {
    let (mut daemon, project, _dir) = daemon("tabs-exit");
    let inbox = subscribed(&mut daemon, 1);
    let (pane, _) = spawn_placed(&mut daemon, &inbox, project, Placement::Auto);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"exit 0\r".to_vec(),
        },
    );

    let seen = wait_for(&mut daemon, &inbox, |m| {
        last_tabs(m, project).is_some_and(|tabs| tabs.is_empty())
    });
    assert!(seen.iter().any(|m| matches!(
        m,
        ServerMessage::PaneChanged {
            update: PaneUpdate::Status {
                status: PaneStatus::Exited(_)
            },
            ..
        }
    )));
    assert_eq!(daemon.pane_count(), 1, "the pane itself stays until closed");
}

/// Every tab snapshot among `messages`, whole, names and all.
fn tab_snapshots(messages: &[ServerMessage]) -> Vec<ServerMessage> {
    messages
        .iter()
        .filter(|m| matches!(m, ServerMessage::Tabs { .. }))
        .cloned()
        .collect()
}

/// Each command tabs brought, aimed at real panes and tabs: a placed spawn,
/// a move, a rename, a reorder and a close.
fn tab_commands(
    project: ProjectId,
    pane: PaneId,
    first: TabId,
    second: TabId,
) -> [ClientMessage; 5] {
    [
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (80, 24),
            place: Placement::Into { tab: first },
            settings: Default::default(),
        },
        ClientMessage::MovePane {
            pane,
            to: Placement::Into { tab: second },
        },
        ClientMessage::RenameTab {
            tab: first,
            name: "taken".into(),
        },
        ClientMessage::MoveTab {
            tab: second,
            index: 0,
        },
        ClientMessage::CloseTab { tab: first },
    ]
}

#[test]
fn no_tab_command_is_acted_on_before_a_hello_or_after_a_refusal() {
    // The tab commands pass through the same gate as every other request:
    // a client that has not been welcomed, or was refused, moves, names,
    // reorders, closes and places nothing.
    let (mut daemon, project, _dir) = daemon("tabs-unwelcome");
    let ui = subscribed(&mut daemon, 1);
    let (pane, _) = spawn_placed(&mut daemon, &ui, project, Placement::Auto);
    let (_, seen) = spawn_placed(&mut daemon, &ui, project, Placement::NewAfter { tab: None });
    let before = tab_snapshots(&seen).pop().expect("the tabs were sent");
    let first = tab_at(&seen, project, 0);
    let second = tab_at(&seen, project, 1);

    let mut id = 10;
    for command in tab_commands(project, pane, first, second) {
        let unwelcome = daemon.attach_for_test(id);
        daemon.request_for_test(id, command.clone());
        assert!(
            drain(&unwelcome)
                .iter()
                .any(|m| matches!(m, ServerMessage::Error { .. })),
            "{command:?} before a Hello is refused, and says why"
        );
        id += 1;

        let refused = daemon.attach_for_test(id);
        daemon.request_for_test(
            id,
            ClientMessage::Hello {
                version: dispatch_proto::Version {
                    major: 99,
                    minor: 0,
                },
                client: "incompatible".into(),
                role: dispatch_proto::Role::Interface,
            },
        );
        daemon.request_for_test(id, command);
        let told = drain(&refused);
        assert!(
            !told.is_empty()
                && told.iter().all(|m| matches!(
                    m,
                    ServerMessage::Error {
                        error: ProtocolError::IncompatibleVersion { .. }
                    }
                )),
            "a refused client is told why, and nothing else: {told:?}"
        );
        id += 1;
    }
    daemon.tick();

    assert_eq!(daemon.pane_count(), 2, "nothing was started or closed");
    assert!(
        tab_snapshots(&drain(&ui)).is_empty(),
        "no tab changed, so none was sent"
    );
    let late = daemon.attach_for_test(3);
    daemon.request_for_test(3, hello());
    daemon.request_for_test(3, ClientMessage::Subscribe);
    assert_eq!(
        tab_snapshots(&drain(&late)),
        vec![before],
        "the tabs are as they were, names and all"
    );
}

#[test]
fn a_client_that_never_reads_is_let_go_of_over_tab_snapshots_too() {
    // Every change to a tab sends every tab the project has to every
    // client, through its outbox like the rest of the fleet's traffic. A
    // client that stopped reading while tabs change is let go once what
    // waits for it passes its budget -- counted for what the snapshots
    // hold, not as so many small messages, which let it hold many times
    // its budget first.
    const BUDGET: usize = 16 * 1024;
    let (mut daemon, project, _dir) = daemon("tabs-unread");
    daemon.set_budgets(Budgets {
        outbox_bytes: BUDGET,
        ..Budgets::default()
    });
    let reading = subscribed(&mut daemon, 1);
    let never_reads = subscribed(&mut daemon, 2);

    let mut seen = Vec::new();
    for _ in 0..4 {
        (_, seen) = spawn_placed(
            &mut daemon,
            &reading,
            project,
            Placement::NewAfter { tab: None },
        );
    }
    let tabs: Vec<TabId> = (0..4).map(|index| tab_at(&seen, project, index)).collect();

    let mut renames = 0;
    while daemon.clients.contains_key(&2) {
        assert!(
            renames < 10_000,
            "a client that never reads was kept through {renames} renames"
        );
        daemon.request_for_test(
            1,
            ClientMessage::RenameTab {
                tab: tabs[renames % tabs.len()],
                name: format!("{renames:>64}"),
            },
        );
        let _ = drain(&reading);
        renames += 1;
    }

    // What was left waiting, as the bytes its writer would have sent.
    let queued: usize = never_reads
        .try_iter()
        .map(|message| {
            let mut frame = Vec::new();
            Frame::write(&mut frame, &message).expect("a message encodes");
            frame.len()
        })
        .sum();
    assert!(
        queued < 2 * BUDGET,
        "{queued} bytes were queued for a client that never read, against a budget of {BUDGET}"
    );
}

#[test]
fn a_pane_placed_on_a_tab_delegates_under_the_same_cap() {
    // Where a pane sits changes nothing about what it may start. Two
    // requests are asked about while nothing runs, a pane of the user's own
    // is placed beside the asker meanwhile, and approving both still starts
    // one subagent: a placed pane is nobody's subagent and frees no slot,
    // and a subagent takes no place on a tab.
    let (mut daemon, project, _dir) = daemon_with_limits(
        "cap-placed",
        DelegationLimits {
            max_depth: 1,
            max_live_per_parent: 1,
            request_timeout_secs: 600,
            ..DelegationLimits::default()
        },
    );
    let ui = subscribed(&mut daemon, 1);
    let (parent, seen) = spawn_placed(&mut daemon, &ui, project, Placement::NewAfter { tab: None });
    let tab = tab_at(&seen, project, 0);

    let first = ask_as(&mut daemon, 8, parent, long_task());
    let second = ask_as(&mut daemon, 9, parent, long_task());
    let requests: Vec<RequestId> = drain(&ui)
        .iter()
        .filter_map(|m| match m {
            ServerMessage::DelegatePending { request, .. } => Some(*request),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 2, "both fit while nothing runs yet");

    let (beside, seen) = spawn_placed(&mut daemon, &ui, project, Placement::Into { tab });
    assert!(
        !seen.iter().any(m_is_child),
        "a placed pane is not anyone's subagent"
    );

    for request in requests {
        daemon.request_for_test(
            1,
            ClientMessage::DelegateDecision {
                request,
                approve: true,
                blanket: false,
            },
        );
    }

    assert_eq!(
        daemon.pane_count(),
        3,
        "the parent, the pane beside it, and exactly one subagent"
    );
    let mut told = outcomes(&drain(&first));
    told.extend(outcomes(&drain(&second)));
    assert_eq!(
        told.iter()
            .filter(|o| matches!(o, DelegateOutcome::Approved { .. }))
            .count(),
        1,
        "one caller is told it runs: {told:?}"
    );
    assert!(
        told.iter()
            .any(|o| matches!(o, DelegateOutcome::Refused { reason } if reason.contains("cap"))),
        "the other is told why it does not: {told:?}"
    );

    let after = drain(&ui);
    assert!(after.iter().any(m_is_child), "the subagent is announced");
    assert_eq!(
        last_tabs(&after, project),
        None,
        "starting a subagent moves no tab"
    );
    assert_eq!(
        last_tabs(&seen, project),
        Some(vec![vec![parent, beside]]),
        "the tab holds the two placed panes"
    );
}

/// Writes `record`: a harness that writes the arguments it was started
/// with to `out`, one to a line, and then waits. `$0` is the file, so what
/// it writes is exactly what its settings added. Its one-shot form writes
/// them and exits.
#[cfg(unix)]
fn write_recording_harness(dir: &std::path::Path, out: &std::path::Path) {
    let body = format!(
        r#"id = "record"
display_name = "Record"
command = "sh"
args = ["-c", "printf '%s\n' \"$@\" > \"$0\"; sleep 30", "{out}"]

[task]
args = ["-c", "printf '%s\n' \"$@\" > \"$0\"", "{out}"]

[[settings]]
key = "model"
label = "Model"
kind = "choice"
options = ["small", "large"]
custom = true
args = ["--model", "{{value}}"]

[[settings]]
key = "effort"
label = "Effort"
kind = "choice"
options = ["low", "high"]
args = ["--effort", "{{value}}"]

[[settings]]
key = "bypass"
label = "Skip prompts"
kind = "bool"
default = true
args = ["--yolo"]
"#,
        out = out.display()
    );
    std::fs::create_dir_all(dir).expect("temp dir is writable");
    std::fs::write(dir.join("record.toml"), body).expect("temp dir is writable");
}

/// A daemon serving `record` beside the usual harnesses, reading saved
/// settings from `dir/config`, plus the file `record` writes to.
#[cfg(unix)]
fn recording_daemon(label: &str) -> (Daemon, ProjectId, TempDir, PathBuf) {
    let dir = TempDir::new(label);
    let harness_dir = dir.0.join("harnesses");
    let out = dir.0.join("record.out");
    write_recording_harness(&harness_dir, &out);
    let registry = harnesses(&harness_dir);

    let mut daemon = Daemon::new(registry, "test-device");
    daemon.set_task_dir(dir.0.join("tasks"));
    daemon.set_settings_dir(dir.0.join("config"));
    let root = dispatch_os::paths::resolve(&dir.0).expect("the temp dir resolves");
    let project = daemon.open_project(root);

    (daemon, project, dir, out)
}

/// Saves `text` as the daemon's `harness-settings.toml`.
#[cfg(unix)]
fn save_settings(dir: &TempDir, text: &str) {
    let config = dir.0.join("config");
    std::fs::create_dir_all(&config).expect("temp dir is writable");
    std::fs::write(config.join("harness-settings.toml"), text).expect("temp dir is writable");
}

/// What `record` wrote, once it has: one argument to a line.
#[cfg(unix)]
fn recorded(daemon: &mut Daemon, out: &std::path::Path) -> Vec<String> {
    let deadline = Instant::now() + WAIT_FOR_DEADLINE;
    loop {
        daemon.tick();
        if let Ok(text) = std::fs::read_to_string(out)
            && text.ends_with('\n')
        {
            return text.lines().map(str::to_string).collect();
        }
        assert!(
            Instant::now() < deadline,
            "record never wrote its arguments"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Asks for a pane of `harness` with `settings`, as a client does.
fn spawn_with(daemon: &mut Daemon, project: ProjectId, harness: &str, settings: &[(&str, &str)]) {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: harness.into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: settings
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        },
    );
}

/// The text of the first `Other` error among `messages`.
fn error_text(messages: &[ServerMessage]) -> Option<String> {
    messages.iter().find_map(|m| match m {
        ServerMessage::Error {
            error: ProtocolError::Other(text),
        } => Some(text.clone()),
        _ => None,
    })
}

#[cfg(unix)]
#[test]
fn a_spawn_starts_with_the_flags_the_client_chose() {
    let (mut daemon, project, _dir, out) = recording_daemon("settings-chosen");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(
        &mut daemon,
        project,
        "record",
        &[("model", "large"), ("bypass", "false")],
    );

    assert_eq!(recorded(&mut daemon, &out), vec!["--model", "large"]);
}

#[cfg(unix)]
#[test]
fn a_spawn_with_nothing_chosen_uses_the_saved_settings_then_the_files() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-saved");
    save_settings(&dir, "[record]\nmodel = \"small\"\n");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(&mut daemon, project, "record", &[]);

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "small", "--yolo"]
    );
}

#[cfg(unix)]
#[test]
fn a_chosen_value_beats_a_saved_one() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-chosen-over-saved");
    save_settings(&dir, "[record]\nmodel = \"small\"\n");
    let _inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());

    spawn_with(&mut daemon, project, "record", &[("model", "large")]);

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "large", "--yolo"]
    );
}

#[test]
fn a_setting_the_harness_does_not_have_is_refused_and_nothing_starts() {
    let (mut daemon, project, _dir) = daemon("settings-unknown");
    let inbox = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    let _ = drain(&inbox);

    spawn_with(&mut daemon, project, "shell", &[("colour", "red")]);

    let text = error_text(&drain(&inbox)).expect("the spawn is refused");
    assert!(text.contains("colour"), "{text}");
    assert_eq!(
        text, "not starting a pane: shell has no setting \"colour\"",
        "{text}"
    );
    assert_eq!(daemon.pane_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_value_the_harness_cannot_take_is_refused_and_nothing_starts() {
    // Refused, not dropped: a dropped `bypass = false` would start an agent
    // with its prompts off.
    for (key, value) in [
        ("model", "x&calc"),
        ("model", "--yolo"),
        ("effort", "extreme"),
        ("bypass", "yes"),
    ] {
        let (mut daemon, project, _dir, _out) = recording_daemon("settings-refused");
        let inbox = daemon.attach_for_test(1);
        daemon.request_for_test(1, hello());
        let _ = drain(&inbox);

        spawn_with(&mut daemon, project, "record", &[(key, value)]);

        let text = error_text(&drain(&inbox))
            .unwrap_or_else(|| panic!("{key} = {value:?} was not refused"));
        assert!(text.contains(key), "{key} = {value:?}: {text}");
        assert_eq!(daemon.pane_count(), 0, "{key} = {value:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_subagent_starts_with_the_saved_settings() {
    let (mut daemon, project, dir, out) = recording_daemon("settings-delegated");
    save_settings(&dir, "[record]\nmodel = \"large\"\n");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "record".into(),
            task: "anything".into(),
            size: (80, 24),
            handoff: Some(handoff_for("anything")),
            interactive: false,
        },
    );
    let request = pending(&drain(&ui)).expect("the interface is asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    assert_eq!(
        recorded(&mut daemon, &out),
        vec!["--model", "large", "--yolo"]
    );
}

/// Attaches interface client `id`, says hello and subscribes.
fn attach_window(daemon: &mut Daemon, id: u64) -> Inbox {
    let inbox = daemon.attach_for_test(id);
    daemon.request_for_test(id, hello());
    daemon.request_for_test(id, ClientMessage::Subscribe);
    inbox
}

/// The last size `messages` said `pane` has.
fn resized(messages: &[ServerMessage], pane: PaneId) -> Option<(u16, u16)> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::PaneResized { pane: p, size } if *p == pane => Some(*size),
        _ => None,
    })
}

fn ask_size(daemon: &mut Daemon, id: u64, pane: PaneId, cols: u16, rows: u16) {
    daemon.request_for_test(
        id,
        ClientMessage::ResizePane {
            pane,
            size: (cols, rows),
        },
    );
}

/// Two windows on one pane: window 1 asked 100×30 and window 2, used
/// last, asked 60×20.
fn two_windows(label: &str) -> (Daemon, TempDir, PaneId, Inbox, Inbox) {
    let (mut daemon, project, dir) = daemon(label);
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    ask_size(&mut daemon, 1, pane, 100, 30);
    let second = attach_window(&mut daemon, 2);
    ask_size(&mut daemon, 2, pane, 60, 20);
    let _ = drain(&first);
    let _ = drain(&second);
    (daemon, dir, pane, first, second)
}

#[test]
fn a_pane_takes_the_size_of_the_window_last_used() {
    let (mut daemon, _dir, pane, first, second) = two_windows("size-last-used");
    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(60, 20)));

    daemon.request_for_test(1, ClientMessage::Active);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), Some((100, 30)));
    assert_eq!(
        resized(&drain(&second), pane),
        Some((100, 30)),
        "the other window is told the real size too"
    );
}

#[test]
fn typing_counts_as_use_and_resizing_does_not() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-typing");

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"x".to_vec(),
        },
    );
    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));

    ask_size(&mut daemon, 2, pane, 70, 25);
    assert_eq!(
        daemon.pane_size_for_test(pane),
        Some(Size::new(100, 30)),
        "a resize is not use: window 1 still decides"
    );
}

#[test]
fn the_next_window_takes_over_when_the_one_in_use_leaves() {
    let (mut daemon, _dir, pane, first, _second) = two_windows("size-leaves");

    daemon.detach_for_test(2);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), Some((100, 30)));
}

#[test]
fn a_window_that_stops_showing_a_pane_stops_sizing_it() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-hides");

    daemon.request_for_test(2, ClientMessage::HidePane { pane });

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
}

#[test]
fn a_pane_nobody_shows_keeps_its_size() {
    let (mut daemon, project, _dir) = daemon("size-nobody");
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    ask_size(&mut daemon, 1, pane, 100, 30);
    let _ = drain(&first);

    daemon.request_for_test(1, ClientMessage::HidePane { pane });

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(100, 30)));
    assert_eq!(resized(&drain(&first), pane), None, "nothing changed");
}

#[test]
fn a_new_pane_is_announced_with_its_size() {
    let (mut daemon, project, _dir) = daemon("size-new-pane");
    let first = attach_window(&mut daemon, 1);
    let _ = drain(&first);

    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: "shell".into(),
            size: (90, 33),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    let seen = drain(&first);
    let spawned = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
        .expect("announced");
    let ServerMessage::PaneSpawned { pane: new, .. } = &seen[spawned] else {
        unreachable!()
    };
    let sized = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneResized { pane: p, .. } if p == new))
        .expect("its size is said");
    assert!(spawned < sized, "after the announcement");
    assert_eq!(resized(&seen, *new), Some((90, 33)));
}

#[test]
fn a_subagent_is_announced_with_its_size() {
    let (mut daemon, project, _dir) = daemon("size-subagent");
    let ui = attach_window(&mut daemon, 1);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    // The delegation helper, which asks at 80×24.
    let _caller = ask(&mut daemon, parent, "echo sized");
    let request = pending(&drain(&ui)).expect("the interface is asked");

    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );

    let seen = drain(&ui);
    let child = seen
        .iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned {
                pane,
                parent: Some(p),
                ..
            } if *p == parent => Some(*pane),
            _ => None,
        })
        .expect("the subagent is announced");
    assert_eq!(resized(&seen, child), Some((80, 24)));
}

#[test]
fn a_window_attaching_is_told_each_panes_size_before_its_output() {
    let (mut daemon, project, _dir) = daemon("size-catch-up");
    let first = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &first, project);
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo size-catch-up-marker\r".to_vec(),
        },
    );
    wait_for(&mut daemon, &first, |m| {
        output_of(m, pane).contains("size-catch-up-marker")
    });

    let second = attach_window(&mut daemon, 2);
    let seen = drain(&second);

    let spawned = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneSpawned { pane: p, .. } if *p == pane))
        .expect("the pane is described");
    let sized = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneResized { pane: p, .. } if *p == pane))
        .expect("its size is said");
    let replayed = seen
        .iter()
        .position(|m| matches!(m, ServerMessage::PaneOutput { pane: p, .. } if *p == pane))
        .expect("its output is replayed");
    assert!(spawned < sized && sized < replayed, "{seen:#?}");
}

#[test]
fn a_delegate_connection_never_decides_a_size() {
    let (mut daemon, _dir, pane, _first, _second) = two_windows("size-delegate");
    let _caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );

    ask_size(&mut daemon, 9, pane, 40, 10);
    daemon.request_for_test(9, ClientMessage::Active);

    assert_eq!(daemon.pane_size_for_test(pane), Some(Size::new(60, 20)));
}

/// Round 3, item 4: with no interface window asking for a pane at all, a
/// delegate connection's own `ResizePane` must still not decide it -- the
/// test above only shows a delegate loses to an interface window that
/// outranks it, not that a delegate is refused outright.
#[test]
fn a_delegate_connection_never_sizes_a_pane_nobody_else_asks_for() {
    let (mut daemon, project, _dir) = daemon("size-delegate-alone");
    let ui = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &ui, project);
    // Withdraws the ask `SpawnPane` recorded on its own, so nothing but the
    // delegate connection below ever asks for this pane's size.
    daemon.request_for_test(1, ClientMessage::HidePane { pane });
    let before = daemon.pane_size_for_test(pane);

    // Kept alive for the rest of the test: dropped, its `Outbox::send_all`
    // for the `Welcome` reply below would fail as `Refused::Gone`, which
    // forgets the client outright and would make the `ResizePane` after it
    // a no-op for a reason that has nothing to do with what this pins.
    let _caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    ask_size(&mut daemon, 9, pane, 40, 10);

    assert_eq!(
        daemon.pane_size_for_test(pane),
        before,
        "a pane no interface window has a size for keeps the size it has, \
         not whatever a delegate connection last asked"
    );
}

#[cfg(unix)]
#[test]
fn a_pane_whose_program_stops_is_closed() {
    // Ctrl Z in a pane whose program is its leader: no shell is behind it to
    // bring it back, so the pane is closed, the program ended, and every
    // window told.
    let (mut daemon, project, _dir) = daemon("stopped-pane");
    let ui = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &ui, project);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"kill -STOP $$\r".to_vec(),
        },
    );

    wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { pane: p } if *p == pane))
    });
    assert_eq!(daemon.pane_count(), 0);
}

#[cfg(unix)]
#[test]
fn a_shell_whose_job_is_stopped_stays_open() {
    // Ctrl Z inside a shell pane stops the job the shell is running, not the
    // shell: the shell takes over again, `fg` works, and the pane stays.
    let (mut daemon, project, _dir) = daemon("stopped-job");
    let ui = attach_window(&mut daemon, 1);
    let pane = spawn_pane_for_test(&mut daemon, &ui, project);

    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"sleep 30\r".to_vec(),
        },
    );
    std::thread::sleep(Duration::from_millis(300));
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: vec![0x1a],
        },
    );
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"echo still-here-marker\r".to_vec(),
        },
    );

    let seen = wait_for(&mut daemon, &ui, |m| {
        output_of(m, pane).matches("still-here-marker").count() >= 2
    });
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { pane: p } if *p == pane)),
        "the shell's own stopped job must not close the pane"
    );
    assert_eq!(daemon.pane_count(), 1);
}

/// Approves the one request pending on `ui` and returns the subagent's pane.
fn approve_and_spawn(daemon: &mut Daemon, ui: &Inbox) -> PaneId {
    let request = pending(&drain(ui)).expect("asked");
    daemon.request_for_test(
        1,
        ClientMessage::DelegateDecision {
            request,
            approve: true,
            blanket: false,
        },
    );
    let seen = wait_for(daemon, ui, |m| m.iter().any(m_is_child));
    seen.iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned {
                pane,
                parent: Some(_),
                ..
            } => Some(*pane),
            _ => None,
        })
        .expect("checked by wait_for")
}

/// Sends a report from `pane` over a fresh delegate connection, and returns
/// that connection's inbox.
fn report_from(daemon: &mut Daemon, id: u64, pane: PaneId, report: &str) -> Inbox {
    let reporter = daemon.attach_for_test(id);
    daemon.request_for_test(
        id,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "report".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        id,
        ClientMessage::DelegateReport {
            pane,
            report: report.into(),
        },
    );
    reporter
}

fn report_outcome(messages: &[ServerMessage]) -> Option<dispatch_proto::ReportOutcome> {
    messages.iter().find_map(|m| match m {
        ServerMessage::ReportAnswered { outcome } => Some(outcome.clone()),
        _ => None,
    })
}

#[test]
fn a_report_answers_the_caller_at_once() {
    let (mut daemon, project, _dir) = daemon("report-delivered");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);

    let reporter = report_from(&mut daemon, 20, child, "the findings");

    assert_eq!(
        report_outcome(&drain(&reporter)),
        Some(dispatch_proto::ReportOutcome::Delivered)
    );
    let finished = drain(&caller).into_iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { exit, report, .. } => Some((exit, report)),
        _ => None,
    });
    assert_eq!(
        finished,
        Some((0, Some("the findings".to_string()))),
        "answered before the still-running subagent exits"
    );
}

#[test]
fn a_one_shot_subagent_that_reported_is_not_killed_when_its_caller_leaves() {
    let (mut daemon, project, _dir) = daemon("report-keeps-pane");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "done");

    // `dispatch delegate` exits the moment it has its answer.
    daemon.detach_for_test(9);
    daemon.tick();

    assert_eq!(
        daemon.pane_count(),
        2,
        "its pane stays for a person to read"
    );
}

#[test]
fn a_second_report_is_refused() {
    let (mut daemon, project, _dir) = daemon("report-twice");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask(&mut daemon, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "first");

    let second = report_from(&mut daemon, 21, child, "second");
    assert!(matches!(
        report_outcome(&drain(&second)),
        Some(dispatch_proto::ReportOutcome::Refused { reason }) if reason.contains("already reported")
    ));
}

#[test]
fn a_pane_that_is_not_a_subagent_cannot_report() {
    let (mut daemon, project, _dir) = daemon("report-not-subagent");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let top = spawn_pane_for_test(&mut daemon, &ui, project);

    let reporter = report_from(&mut daemon, 20, top, "unasked");
    assert!(matches!(
        report_outcome(&drain(&reporter)),
        Some(dispatch_proto::ReportOutcome::Refused { reason }) if reason.contains("not a subagent")
    ));
}

#[test]
fn a_report_nobody_is_waiting_for_says_so() {
    let (mut daemon, project, _dir) = daemon("report-no-caller");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask(&mut daemon, parent, "echo quick");
    let child = approve_and_spawn(&mut daemon, &ui);
    // The one-shot exits and its caller is answered with the tail: the point
    // at which nobody is waiting any more.
    wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });

    let late = report_from(&mut daemon, 20, child, "too late");
    assert!(matches!(
        report_outcome(&drain(&late)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
}

#[test]
fn a_report_from_an_unknown_pane_is_an_error() {
    let (mut daemon, _project, _dir) = daemon("report-unknown");
    let reporter = report_from(&mut daemon, 20, PaneId::new(), "who");
    assert!(drain(&reporter).iter().any(|m| matches!(
        m,
        ServerMessage::Error {
            error: ProtocolError::NoSuchPane(_)
        }
    )));
}

#[test]
fn a_report_over_the_limit_is_cut_on_a_character_boundary() {
    // Three-byte characters, so the limit falls inside one.
    let report = "€".repeat(MAX_REPORT_BYTES / 3 + 10);
    let cut = cut_report(report);
    assert!(cut.len() <= MAX_REPORT_BYTES + 200);
    assert!(cut.ends_with("[dispatch] the report was cut at 1 MiB\n"));
    assert!(cut.starts_with('€'));
}

#[test]
fn the_cap_counts_requests_still_open_not_processes_still_running() {
    let limits = DelegationLimits {
        max_live_per_parent: 1,
        ..DelegationLimits::default()
    };
    let (mut daemon, project, _dir) = daemon_with_limits("report-cap", limits);
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _first = ask_as(&mut daemon, 9, parent, long_task());
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "done");

    // The first is still running, but its request is finished.
    let _second = ask_as(&mut daemon, 10, parent, "echo second");
    assert!(
        pending(&drain(&ui)).is_some(),
        "a reported subagent no longer holds its parent's slot"
    );
}

/// Attaches a delegate caller and asks for an interactive subagent.
///
/// Unix only, like every test that uses it: on Windows it would be dead code.
#[cfg(unix)]
fn ask_interactive(daemon: &mut Daemon, parent: PaneId, goal: &str) -> Inbox {
    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "shell".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(handoff_for(goal)),
            interactive: true,
        },
    );
    caller
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_closes_once_it_has_reported() {
    let (mut daemon, project, _dir) = daemon("interactive-close");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    let _ = report_from(&mut daemon, 20, child, "interactive findings");

    let answer = drain(&caller).into_iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { report, .. } => report,
        _ => None,
    });
    assert_eq!(answer.as_deref(), Some("interactive findings"));
    assert!(
        drain(&ui)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { pane } if *pane == child)),
        "closed after the caller had its answer"
    );
}

#[cfg(unix)]
#[test]
fn under_ask_a_reported_interactive_subagent_stays_and_is_marked() {
    let limits = DelegationLimits {
        interactive_on_done: dispatch_config::OnDone::Ask,
        ..DelegationLimits::default()
    };
    let (mut daemon, project, _dir) = daemon_with_limits("interactive-ask", limits);
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    let _ = report_from(&mut daemon, 20, child, "done");

    assert!(
        drain(&ui)
            .iter()
            .any(|m| matches!(m, ServerMessage::SubagentReported { pane } if *pane == child))
    );
    assert_eq!(daemon.pane_count(), 2, "kept for the user to close");

    // A window attaching later is told too.
    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    assert!(
        drain(&late)
            .iter()
            .any(|m| matches!(m, ServerMessage::SubagentReported { pane } if *pane == child))
    );
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_outlives_a_caller_that_left_before_it_reported() {
    let (mut daemon, project, _dir) = daemon("interactive-orphan");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    daemon.detach_for_test(9);
    daemon.tick();
    assert_eq!(daemon.pane_count(), 2, "never ended under the user");

    let late = report_from(&mut daemon, 20, child, "nobody");
    assert!(matches!(
        report_outcome(&drain(&late)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
    assert_eq!(
        daemon.pane_count(),
        2,
        "and not closed by a report nobody wanted"
    );
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_that_exits_without_reporting_answers_with_no_tail() {
    let (mut daemon, project, _dir) = daemon("interactive-no-report");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "echo screen-noise");
    let _child = approve_and_spawn(&mut daemon, &ui);

    let seen = wait_for(&mut daemon, &caller, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { .. }))
    });
    let finished = seen.iter().find_map(|m| match m {
        ServerMessage::DelegateFinished { tail, report, .. } => {
            Some((tail.clone(), report.clone()))
        }
        _ => None,
    });
    assert_eq!(
        finished,
        Some((Vec::new(), None)),
        "a full-screen tail is not output"
    );
}

#[cfg(unix)]
#[test]
fn closing_an_interactive_subagent_before_it_reports_answers_its_caller() {
    let (mut daemon, project, _dir) = daemon("interactive-closed");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: child });

    assert!(
        drain(&caller)
            .iter()
            .any(|m| matches!(m, ServerMessage::DelegateFinished { exit: -1, .. }))
    );
}

#[test]
fn a_harness_without_an_interactive_form_refuses_interactive() {
    let (mut daemon, project, _dir) = daemon("interactive-none");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);

    let caller = daemon.attach_for_test(9);
    daemon.request_for_test(
        9,
        ClientMessage::Hello {
            version: dispatch_proto::VERSION,
            client: "delegate".into(),
            role: dispatch_proto::Role::Delegate,
        },
    );
    daemon.request_for_test(
        9,
        ClientMessage::DelegateRequest {
            parent,
            harness: "no-task-args".into(),
            task: String::new(),
            size: (80, 24),
            handoff: Some(handoff_for("echo x")),
            interactive: true,
        },
    );
    let reason = refusal(&drain(&caller)).expect("refused");
    assert!(reason.contains("[task.interactive]"), "{reason}");
}

/// A daemon under `interactive_on_done = "ask"` with a window subscribed as
/// client 1, an interactive subagent that has reported, and the window's
/// inbox drained.
#[cfg(unix)]
fn reported_under_ask(label: &str) -> (Daemon, TempDir, Inbox, PaneId) {
    let limits = DelegationLimits {
        interactive_on_done: dispatch_config::OnDone::Ask,
        ..DelegationLimits::default()
    };
    let (mut daemon, project, dir) = daemon_with_limits(label, limits);
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);
    let _ = report_from(&mut daemon, 20, child, "done");
    assert!(
        drain(&ui)
            .iter()
            .any(|m| matches!(m, ServerMessage::SubagentReported { pane } if *pane == child))
    );
    (daemon, dir, ui, child)
}

#[cfg(unix)]
#[test]
fn keeping_a_reported_subagent_tells_every_window() {
    let (mut daemon, _dir, ui, child) = reported_under_ask("keep-reported");
    let other = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    let _ = drain(&other);

    daemon.request_for_test(1, ClientMessage::KeepReported { pane: child });

    for inbox in [&ui, &other] {
        assert!(
            drain(inbox)
                .iter()
                .any(|m| matches!(m, ServerMessage::SubagentKept { pane } if *pane == child)),
            "every window hears it was kept"
        );
    }
    assert_eq!(daemon.pane_count(), 2, "kept, not closed");
}

#[cfg(unix)]
#[test]
fn a_window_attaching_after_a_keep_is_not_asked_again() {
    let (mut daemon, _dir, _ui, child) = reported_under_ask("keep-then-attach");
    daemon.request_for_test(1, ClientMessage::KeepReported { pane: child });

    let late = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);
    assert!(
        !drain(&late)
            .iter()
            .any(|m| matches!(m, ServerMessage::SubagentReported { .. })),
        "the user already answered"
    );

    // Kept for good: a second report is still refused.
    let again = report_from(&mut daemon, 21, child, "again");
    assert!(matches!(
        report_outcome(&drain(&again)),
        Some(dispatch_proto::ReportOutcome::Refused { reason }) if reason.contains("already reported")
    ));
}

#[cfg(unix)]
#[test]
fn keeping_a_pane_that_is_not_waiting_is_ignored() {
    let (mut daemon, project, _dir) = daemon("keep-not-waiting");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let top = spawn_pane_for_test(&mut daemon, &ui, project);
    let _ = drain(&ui);

    daemon.request_for_test(1, ClientMessage::KeepReported { pane: top });

    assert!(
        !drain(&ui).iter().any(|m| matches!(
            m,
            ServerMessage::SubagentKept { .. } | ServerMessage::Error { .. }
        )),
        "nothing to keep, and nothing wrong with asking"
    );
}

#[test]
fn keeping_an_unknown_pane_is_an_error() {
    let (mut daemon, _project, _dir) = daemon("keep-unknown");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _ = drain(&ui);

    daemon.request_for_test(
        1,
        ClientMessage::KeepReported {
            pane: PaneId::new(),
        },
    );

    assert!(drain(&ui).iter().any(|m| matches!(
        m,
        ServerMessage::Error {
            error: ProtocolError::NoSuchPane(_)
        }
    )));
}

#[cfg(unix)]
#[test]
fn a_report_to_a_caller_that_has_gone_leaves_the_pane_alone() {
    let (mut daemon, project, _dir) = daemon("report-caller-gone");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    // The caller's connection ends, and a reply to it fails before its
    // detach is handled: the daemon forgets the client without abandoning
    // what it asked for, so the pane still names it.
    drop(caller);
    daemon.request_for_test(9, ClientMessage::Ping { token: 1 });

    let late = report_from(&mut daemon, 20, child, "nobody");
    assert!(matches!(
        report_outcome(&drain(&late)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
    assert_eq!(daemon.pane_count(), 2, "not closed by a report nobody got");
    let _ = drain(&ui);

    // Not marked reported either: once the detach is handled, a report it
    // sends is told the same, not that it already reported.
    daemon.detach_for_test(9);
    daemon.tick();
    let again = report_from(&mut daemon, 21, child, "still nobody");
    assert!(matches!(
        report_outcome(&drain(&again)),
        Some(dispatch_proto::ReportOutcome::NotWaiting { .. })
    ));
}

#[cfg(unix)]
#[test]
fn an_interactive_subagent_outlives_its_parents_pane() {
    let (mut daemon, project, _dir) = daemon("interactive-parent-closed");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let parent = spawn_pane_for_test(&mut daemon, &ui, project);
    let _caller = ask_interactive(&mut daemon, parent, "sleep 30");
    let child = approve_and_spawn(&mut daemon, &ui);

    daemon.request_for_test(1, ClientMessage::ClosePane { pane: parent });

    assert!(
        !drain(&ui)
            .iter()
            .any(|m| matches!(m, ServerMessage::PaneClosed { pane } if *pane == child)),
        "the user may be working in it"
    );
    assert_eq!(daemon.pane_count(), 1, "the parent went, the child stayed");
    assert!(daemon.pane_size_for_test(child).is_some());
}

/// Spawns a pane of `harness` for client 1 and returns its id.
fn spawn_harness(daemon: &mut Daemon, inbox: &Inbox, project: ProjectId, harness: &str) -> PaneId {
    daemon.request_for_test(
        1,
        ClientMessage::SpawnPane {
            project,
            harness: harness.into(),
            size: (80, 24),
            place: Placement::Auto,
            settings: Default::default(),
        },
    );
    let seen = wait_for(daemon, inbox, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::PaneSpawned { .. }))
    });
    seen.iter()
        .find_map(|m| match m {
            ServerMessage::PaneSpawned { pane, .. } => Some(*pane),
            _ => None,
        })
        .expect("a pane was spawned")
}

#[cfg(unix)]
#[test]
fn a_program_that_asks_its_terminal_gets_exactly_one_answer() {
    let (mut daemon, project, _dir) = daemon("answer-once");
    let first = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let second = daemon.attach_for_test(2);
    daemon.request_for_test(2, hello());
    daemon.request_for_test(2, ClientMessage::Subscribe);

    let pane = spawn_harness(&mut daemon, &first, project, "asker");
    let seen = wait_for(&mut daemon, &first, |m| {
        output_of(m, pane).contains("EXTRA:")
    });
    let _ = drain(&second);

    let output = output_of(&seen, pane);
    assert!(output.contains("E[?62;22c"), "{output:?}");
    assert!(
        output.contains("EXTRA:0"),
        "one answer, not one per window: {output:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_program_is_answered_with_no_window_watching() {
    let (mut daemon, project, _dir) = daemon("answer-unwatched");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_harness(&mut daemon, &ui, project, "late-asker");

    // Every window gone before the question is printed.
    daemon.detach_for_test(1);
    let deadline = Instant::now() + WAIT_FOR_DEADLINE;
    loop {
        daemon.tick();
        let history = daemon
            .pane_history_for_test(pane)
            .expect("the pane is still there");
        if String::from_utf8_lossy(&history).contains("EXTRA:") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the pane never finished printing"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // A window arriving now is replayed what the pane printed.
    let late = daemon.attach_for_test(3);
    daemon.request_for_test(3, hello());
    daemon.request_for_test(3, ClientMessage::Subscribe);
    let output = output_of(&drain(&late), pane);
    assert!(output.contains("E[?62;22c"), "{output:?}");
    assert!(
        output.contains("EXTRA:0"),
        "replay causes no second answer: {output:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_answered_size_follows_a_resize() {
    let (mut daemon, project, _dir) = daemon("answer-resize");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_harness(&mut daemon, &ui, project, "sizer");

    daemon.request_for_test(
        1,
        ClientMessage::ResizePane {
            pane,
            size: (100, 30),
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        m.iter().any(|m| {
            matches!(
                m,
                ServerMessage::PaneResized {
                    size: (100, 30),
                    ..
                }
            )
        })
    });
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"x".to_vec(),
        },
    );

    let seen = wait_for(&mut daemon, &ui, |m| output_of(m, pane).contains("E[8;"));
    let output = output_of(&seen, pane);
    assert!(output.contains("E[8;30;100t"), "{output:?}");
}

#[cfg(unix)]
#[test]
fn a_resize_report_the_answerer_produces_reaches_the_program() {
    let (mut daemon, project, _dir) = daemon("answer-resize-report");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let pane = spawn_harness(&mut daemon, &ui, project, "reporter");
    // The byte it waits for, so the mode goes on after the spawn's batch.
    daemon.request_for_test(
        1,
        ClientMessage::WritePane {
            pane,
            bytes: b"x".to_vec(),
        },
    );
    // The mode has to be on, and seen to be, before the resize.
    wait_for(&mut daemon, &ui, |m| output_of(m, pane).contains("[?2048h"));

    daemon.request_for_test(
        1,
        ClientMessage::ResizePane {
            pane,
            size: (100, 30),
        },
    );
    wait_for(&mut daemon, &ui, |m| {
        output_of(m, pane).contains("E[48;30;100;0;0t")
    });
}

#[cfg(unix)]
#[test]
fn a_program_that_floods_queries_and_never_reads_does_not_stall_the_daemon() {
    let (mut daemon, project, _dir) = daemon("answer-flood");
    let ui = daemon.attach_for_test(1);
    daemon.request_for_test(1, hello());
    daemon.request_for_test(1, ClientMessage::Subscribe);
    let _pane = spawn_harness(&mut daemon, &ui, project, "flooder");

    let started = Instant::now();
    for _ in 0..300 {
        daemon.tick();
    }
    daemon.request_for_test(1, ClientMessage::Ping { token: 7 });
    let seen = wait_for(&mut daemon, &ui, |m| {
        m.iter()
            .any(|m| matches!(m, ServerMessage::Pong { token: 7 }))
    });
    assert!(!seen.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the loop kept turning"
    );
}
