# Turtle

[![GitHub Action](https://img.shields.io/github/workflow/status/raviqqe/turtle/test?style=flat-square)](https://github.com/raviqqe/turtle/actions)
[![License](https://img.shields.io/github/license/raviqqe/turtle.svg?style=flat-square)](LICENSE)

Clone of the [Ninja build system](https://github.com/ninja-build/ninja) in Rust

## Install

```sh
cargo install --git https://github.com/raviqqe/turtle
```

## Usage

```sh
turtle
```

For more information, see `turtle --help`.

## Compatibility

Turtle aims to support full syntax of the Ninja build files.

- Syntax
  - [x] `build` statement
    - [ ] `phony` rule
  - [x] `rule` statement
  - [x] `default` statement
  - [x] `include` statement
  - [x] `subninja` statement
  - [x] Global variables
  - [x] Build-local variables
  - [x] `in` and `out` special variable
  - [ ] `builddir` special variable
- Command line arguments
  - [x] `-f` custom build file option
  - [ ] `-j` job limit option
  - [ ] `-k` keep-going option
- Others
  - [ ] Circular output dependency detection (#15)
  - [ ] Circular build file dependency detection (#19)
  - [ ] Windows support

For more information, see [issues](https://github.com/raviqqe/turtle/issues).

## License

[MIT](LICENSE)
