# The daemon and other machines

How `dispatchd` keeps agents running past the window, and how one sidebar reaches agents on other machines over ssh.

## Running the daemon

Dispatch works on its own, with the agents as its children. Started that way,
closing it closes them.

`dispatchd` owns the agents instead, so they survive a client exiting. `--attach`
starts one if none is listening, so the daemon stays an implementation detail:

```sh
dispatch --attach /path/to/project             # starts a daemon if needed
dispatch --attach --no-start                   # or insist on one already there
dispatchd /path/to/project                     # or run it yourself
```

A daemon a client starts is detached from that client's terminal: it keeps
running when the client exits, and a Ctrl-C meant for the interface does not
reach the agents. It records its process id in `dispatchd.pid` beside the socket,
which is what to stop when you want it gone.

Several clients can attach at once and see the same panes. Each pane takes the
size of the window you used last: the one you typed or clicked in, resized, or
opened most recently. The other windows show it at that size, with blank space
around it or its edges cut off to fit their tile. A client attaching to a pane
that is already running is replayed the last 256 KiB it printed, so reattaching
shows the work rather than a blank rectangle.

Moving the pointer over a window does not count as using it, so a forgotten
window cannot resize every agent as the pointer crosses it. To have the window
under the pointer take the panes instead:

```toml
# ~/.config/dispatch/config.toml
[interface]
hover_claims_panes = true
```

An attached client reconnects on its own: restart the daemon, or lose the socket,
and it waits, says so, and rebuilds its view from what the daemon reports when it
answers again. A connection that goes quiet is asked whether it is still there,
so a socket that is up but carrying nothing is noticed rather than waited on.

The daemon runs in the foreground and logs to a file. Projects given on its
command line are served immediately; an attached client opens more over the
socket. One daemon per configuration directory: a second refuses to start rather
than splitting the fleet in two. `SIGTERM`, `SIGINT`, or a closed console stops
it and terminates its panes. `DISPATCH_CONFIG_DIR` gives a separate daemon its
own endpoint, harnesses, and log.

## More than one machine

Register a machine once, and every Dispatch after that reaches it over ssh:

```sh
dispatch machine add me@tower          # dials it first; saved only if it answers
dispatch machine add gpu-box --name gpu
dispatch machine list
dispatch machine remove gpu            # its daemon and agents keep running
```

The machine needs `dispatchd` on its `PATH`; nothing is copied to it. Dispatch
runs `ssh -T -o BatchMode=yes -o ConnectTimeout=10 <target> dispatchd --stdio`,
so ssh never prompts: run `ssh <target>` once by hand first to accept its host
key, and use a key or an agent rather than a password. Anything else — a
wrapper, a nix shell, a transport other than ssh — goes after `--`:

```sh
dispatch machine add gpu-box -- /opt/tools/tunnel gpu-box dispatchd --stdio
```

With any machine registered, `dispatch` attaches to its daemons on its own —
this machine's included — and draws a row for each at once. Every project is
drawn under the machine it is on. A machine that is asleep, or whose daemon
has gone down since, stays in the sidebar — dimmed and labelled
`unreachable` — and joins (or rejoins) when it answers again; keystrokes
aimed at it are refused rather than swallowed. Its agents are not lost in the
meantime: they belong to the daemon on that machine, not to the connection to
it, so a dropped connection costs the view and nothing else, and the client
redials until the panes are there again. `^a m` adds a machine without
restarting. `^a o` asks which machine to open a project on; a remote one takes
a typed path, such as `~/code/app`.

`--daemon <endpoint>` and `--daemon-command "<command>"` still reach a daemon
for one run without registering it. `--daemon-command` is split on
whitespace, with no shell; a program whose path holds a space needs the
registry.

[Back to the README](../README.md)
