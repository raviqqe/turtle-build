# Turtle

[![GitHub Action](https://img.shields.io/github/workflow/status/raviqqe/turtle/test?style=flat-square)](https://github.com/raviqqe/turtle/actions)
[![License](https://img.shields.io/badge/license-MIT%20%2B%20Apache%202.0-yellow?style=flat-square)](#License)

Clone of [the Ninja build system](https://github.com/ninja-build/ninja) written in Rust

## Goals

- Naive but safe (no `unsafe`) reimplementation of the Ninja build system in Rust
- Improved frontend support (WIP)
  - Full output from build rules and no output from Turtle by default
  - More customizable build/rule/progress/error output

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

## Compatibility

Turtle aims to support full syntax of the Ninja build files. It also supports basic command line arguments but is not going to implement all the original options.

### Syntax

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

### Command line arguments

- [x] `-f` custom build file option
- [x] `-j` job limit option
- [ ] `-k` keep-going option
- [ ] `-C` change-directory option

### Others

- [x] Circular build dependency detection
- [x] Circular build file dependency detection
- [x] `builddir` special variable
- [x] Dynamic dependencies
  - [x] Implicit inputs
  - [ ] Implicit outputs
  - [ ] Circular build dependency detection
- [ ] C/C++ header dependencies
  - [ ] `depfile` option
  - [ ] `deps` option
- [ ] Windows support

## Similar projects

- [`ninja-rs/ninja-rs`](https://github.com/ninja-rs/ninja-rs)
- [`nikhilm/ninja-rs`](https://github.com/nikhilm/ninja-rs)

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).
