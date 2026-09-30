# Building and contributing

Building every crate, offline and Windows builds, the checks CI runs, and what each crate does.

## Building from source

`cargo build --workspace` builds everything; the
[requirements in the README](../README.md#install) apply. The first build runs `zig build` against
`vendor/libghostty-vt`, which fetches Zig dependencies into
`vendor/libghostty-vt/zig-pkg/` (gitignored). For an offline or hermetic
build, point Zig at a pre-fetched package set:

```sh
LIBGHOSTTY_VT_ZIG_SYSTEM_DIR=/path/to/packages cargo build --workspace
```

On Windows, build for the GNU ABI so Zig supplies its own MinGW libc and no
Visual Studio install is needed:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --workspace --target x86_64-pc-windows-gnu
```

CI builds with exactly Rust 1.89, so a change that needs a newer compiler fails
there first. Before sending a change, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

| Crate | Responsibility |
|---|---|
| `dispatch-core` | Domain types and state. Zero I/O. |
| `dispatch-layout` | Tiling algorithm. Pure functions. |
| `dispatch-config` | Harness definitions, config loading. |
| `dispatch-os` | All platform-specific code. The only crate with `#[cfg(windows)]`. |
| `dispatch-pty` | PTY supervision, VT screen state, title scanning. |
| `dispatch-proto` | The client-daemon wire protocol. |
| `dispatch-client` | The client half of that protocol. |
| `dispatch-daemon` | The daemon's loop: it owns the agents. |
| `dispatch-tui` | Rendering, input routing, keymap. |
| `dispatch` | The client binary. |
| `dispatchd` | The daemon binary. |
| `xtask` | Build tooling — regenerates FFI bindings on version bumps. |

[Back to the README](../README.md)
