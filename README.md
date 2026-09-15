# Turtle

[![GitHub Action](https://img.shields.io/github/actions/workflow/status/raviqqe/turtle-build/test.yaml?branch=main&style=flat-square)](https://github.com/raviqqe/turtle-build/actions)
[![crates.io](https://img.shields.io/crates/v/turtle-build?style=flat-square)](https://crates.io/crates/turtle-build)
[![License](https://img.shields.io/crates/l/turtle-build?style=flat-square)](#license)

[Ninja][ninja]-compatible build system for better end-user experience written in Rust.

## Goals

- Memory safe implementation of the Ninja build system in Rust
- Modest, comprehensible, and customizable build/error outputs
  - Turtle never shows any information that is inscrutable to end-users.
  - This is important for users of high-level programming languages who do not know how compilers and build systems work.

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
- Content hash-based rebuild
- Description-only outputs
  - Turtle never shows commands of build rules but only descriptions because they are incomprehensible to end-users.
- Source mapping
  - Turtle maps outputs in error messages to source filenames defined as `srcdep` variables defined in `build` directives to make them understandable to end-users.
- `--log-prefix` option
  - It changes log prefixes attached to every line of logs from Turtle itself (e.g. `--log-prefix my-build-system` for a log of `my-build-system: build failed`.)
- `--quiet` option
  - It suppresses error messages from Turtle itself on expected build errors. This is useful when you are spawning Turtle as a child process of some higher-level build system.
- Console output handling similar to Rust's Cargo
  - Turtle shows outputs of build jobs running currently at the bottom of logs. So it's easy to track what is going on during builds.

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
- [x] `in_newline` special variable

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
- [x] C/C++ header dependencies
  - [x] `depfile` option
  - [x] `deps` option
- [ ] `rspfile` and `rspfile_content` options
- [ ] Windows support

## Technical notes

Unlike traditional build systems, Turtle parallelizes builds naturally in a way similar to parallel graph reduction, where a graph is rewritten in parallel until it is reduced to a final result. This is made possible by Rust's ecosystem of futures and stackless coroutines.

Here is how parallel builds work in Turtle:

1. Turtle spawns futures for all builds of default targets.
2. Depending on builds' configuration, they spawn more futures or resolve their futures.
   - If they require some input targets to be built first, they spawn those builds for input targets all in parallel.
3. Those futures are scheduled and run in parallel by an asynchronous runtime in Rust.
4. Builds complete when all the futures are resolved.

Currently, Turtle uses a topological sort algorithm only to detect dependency cycles but not for scheduling of build jobs.

## Similar projects

- [`evmar/n2`](https://github.com/evmar/n2)
- [`ninja-rs/ninja-rs`](https://github.com/ninja-rs/ninja-rs)
- [`nikhilm/ninja-rs`](https://github.com/nikhilm/ninja-rs)
- [`neul-labs/rninja`](https://github.com/neul-labs/rninja)

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[ninja]: https://github.com/ninja-build/ninja
