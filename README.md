# Turtle

[![GitHub Action](https://img.shields.io/github/workflow/status/raviqqe/turtle/test?style=flat-square)](https://github.com/raviqqe/turtle/actions)
[![License](https://img.shields.io/github/license/raviqqe/turtle.svg?style=flat-square)](LICENSE)

Clone of [Ninja build system](https://github.com/ninja-build/ninja) in Rust

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

Currently, Turtle only supports a subset of the Ninja build file syntax. See [issues](https://github.com/raviqqe/turtle/issues) for more information.

The subset syntax is fully declarative differently from the original Ninja syntax, where:

- Different types of statements cannot be mixed.
- In each build file, statements are declared in the order of:
  - `include` statement (WIP)
  - Variable definitions
  - `rule` statements
  - `build` statements
  - `default` statements (WIP)
  - `subninja` statements

## License

[MIT](LICENSE)
