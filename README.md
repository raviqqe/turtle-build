# Turtle

[![GitHub Action](https://img.shields.io/github/workflow/status/raviqqe/turtle/test?style=flat-square)](https://github.com/raviqqe/turtle/actions)
[![crates.io](https://img.shields.io/crates/v/turtle-build?style=flat-square)](https://crates.io/crates/turtle-build)
[![License](https://img.shields.io/crates/l/turtle-build?style=flat-square)](#License)

[Ninja][ninja]-compatible build system for high-level programming languages written in Rust

## Goals

- Safe (no `unsafe`) and fast implementation of the Ninja build system in Rust
- Modest, comprehensive, and customizable build/error outputs
  - Turtle doesn't show any information that is not understandable to end-users.
  - This is important for users of high-level programming languages who do not know how compilers and build systems work.

Turtle is originally written for [the Pen programming language](https://github.com/pen-lang/pen). Therefore, we support only dynamic dependencies but not C/C++ header dependencies currently. Your contribution is welcome! 😄

## Install

```sh
cargo install turtle-build
```

## Usage

```sh
turtle
```

For more information, see `turtle --help`.

## Features

- [Ninja][ninja]-compatible build file syntax and command line options 🥷
- `--quiet` option
  - It suppresses error messages from Turtle itself on expected build errors. This is useful when you are spawning Turtle as a child process of a higher-level build system.
- `--log-prefix` option
  - It changes log prefixes attached to every line of logs from Turtle itself (e.g. `--log-prefix my-build-system` for `my-build-system: build failed`)
- Source mapping
  - Turtle maps outputs in error messages to source filenames defined in `srcdep` variables defined in `build` directives to make them understandable to end-users. 
- Console output handling similar to Rust's Cargo
  - Turtle shows outputs of build jobs running currently. So it's easy to track what is going on during builds.

### Compatibility with [Ninja][ninja]

Turtle aims to support full syntax of the Ninja build files. It also supports basic command line arguments but is not going to implement all the original options (e.g. `-t` option.)

#### Syntax

- [x] `build` statement
  - [x] Explicit outputs
  - [x] Explicit inputs
  - [x] Implicit outputs
  - [x] Implicit inputs
  - [x] Order-only inputs
  - [x] `phony` rule
- [x] `rule` statement
- [x] `default` statement
- [x] `include` statement
- [x] `subninja` statement
- [ ] `pool` statement
- [x] Global variables
- [x] Build-local variables
- [x] `in` and `out` special variable

#### Command line arguments

- [x] `-f` custom build file option
- [x] `-j` job limit option
- [ ] `-k` keep-going option
- [x] `-C` change-directory option

#### Others

- [x] Circular build dependency detection
- [x] Circular build file dependency detection
- [x] `builddir` special variable
- [x] Dynamic dependencies
  - [x] Implicit inputs
  - [ ] Implicit outputs
  - [x] Circular build dependency detection
- [ ] C/C++ header dependencies
  - [ ] `depfile` option
  - [ ] `deps` option
- [ ] Windows support

## Technical notes

Something different from the traditional build systems and notable in Turtle is that it solves parallel builds similar to parallel graph reduction naturally, where you modify graph structures in parallel and reduce it into a solution, thanks to an ecosystem of futures and stackless coroutines in Rust.

Here is how parallel builds work in Turtle:

1. Turtle spawns futures for all builds of default targets.
2. Depending on builds' configuration, they spawn more futures or resolve their futures.
   - If they require some input targets to be built first, they spawn those builds for input targets all in parallel.
3. Those futures are scheduled and run in parallel by an asynchronous runtime in Rust.
4. Builds complete when all the futures are resolved.

Currently, Turtle uses a topological sort algorithm only to detect dependency cycles but not for scheduling of build jobs.

Turtle is powered by the following neat projects and others!

- [tokio: Asynchronous runtime for Rust](https://github.com/tokio-rs/tokio)
- [sled: Embedded database in Rust](https://github.com/spacejam/sled)
- [petgraph: Graph algorithms in Rust](https://github.com/petgraph/petgraph)

## Similar projects

- [`ninja-rs/ninja-rs`](https://github.com/ninja-rs/ninja-rs)
- [`nikhilm/ninja-rs`](https://github.com/nikhilm/ninja-rs)

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[ninja]: https://github.com/ninja-build/ninja
