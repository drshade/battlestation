# Rust toolchain (for building bsctl)

`ctl/` (the bsctl binary — see AGENTS.md "bsctl owns stateful protocols") is
the one part of this repo that needs a compiler. The toolchain manager is
declared in `packages/pacman.txt` (`rustup`); the only out-of-band step is
picking a toolchain once — rustup ships no default:

```sh
rustup default stable
```

Then `make build` compiles and installs `~/.local/bin/bsctl` (doctor fails
when the deployed binary is stale against `ctl/`; `make fix` rebuilds).

**History / why pacman-rustup:** the machine originally had a curl-installed
rustup (`sh.rustup.rs`) — installed 2026-06-29 not by hand but by a Claude
Code session working in another project, invisible to the package manifest.
Replaced 2026-07-03 with Arch's `rustup` package (shims in `/usr/bin`, no
`~/.cargo/env.fish` sourcing needed — the old `fish/conf.d/rustup.fish` was
removed; rustup's uninstaller had emptied it through the stow symlink).
Toolchains still live per-user under `~/.rustup`.
