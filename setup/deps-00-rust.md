# Rust toolchain (for building bsctl)

`ctl/` (the bsctl binary — see AGENTS.md "bsctl owns stateful protocols") is
the one part of this repo that needs a compiler. `rustup` is declared in
`packages/pacman.txt`; it ships no default toolchain, so pick one once:

```sh
rustup default stable
```

Then `make build` compiles and installs `~/.local/bin/bsctl` (doctor fails
when the deployed binary is stale against `ctl/`; `make fix` rebuilds).
