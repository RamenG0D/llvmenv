## :sos: This is not the original project, the original is in a state of little or no development, this is the [original](https://github.com/llvmenv/llvmenv). So I forked it to fix some issues and use it

# llvmenv

[![crate](https://img.shields.io/crates/v/llvmenv.svg)](https://crates.io/crates/llvmenv)
[![docs.rs](https://docs.rs/llvmenv/badge.svg)](https://docs.rs/llvmenv)

Manage multiple LLVM/Clang build

## Install

if you are on arch the skip this and just go to [arch](#arch-install)

0. Install cmake, builder (make/ninja), and C++ compiler (g++/clang++)
1. Install Rust using [rustup](https://github.com/rust-lang-nursery/rustup.rs) or any other method.  The minimum supported Rust version is currently **1.48.0**.
2. `cargo install llvmenv --git "https://github.com/RamenG0D/llvmenv.git"`

## arch-install

just run

```shell
git clone https://github.com/RamenG0D/llvmenv.git
cd llvmenv
cargo install arch
cargo arch -si
```

this installs the package using makepkg :D

### Basic Usage

To install a specific version of LLVM after following the installation steps above, run these shell commands ("10.0.0" can be replaced with any other version found with `llvmenv entries`):

```
llvmenv init
llvmenv entries
llvmenv build-entry 10.0.0
```

## shell completions

To get auto completions in your shell (bash, zsh, fish, etc.) you can use the command

```shell
llvmenv completions > [PATH_TO_COMPLETION_SCRIPTS]/[SCRIPT_NAME]
```

**IMPORTANT** please check what the SHELL env var is and ensure that matches the current shell you are currently using
you can do this via

```shell
echo $SHELL
```

so for zsh the command is

```shell
llvmenv completions > $HOME/.oh-my-zsh/completions/_llvmenv
```

* ***Note*** this only works if you have [oh-my-zsh](https://github.com/ohmyzsh/ohmyzsh) installed

to use this without oh-my-zsh find a directory where completions are checked for by running

```shell
echo $FPATH
llvmenv completions > [one of the above directories]/_llvmenv
```

and for bash it would be

```bash
llvmenv completions | sudo tee /usr/share/bash-completion/completions/llvmenv > /dev/null
```

## zsh integration

* This is only for dynamically changing the env based on the llvm builds (not [shell completion](#shell-completions)'s)

You can swtich LLVM/Clang builds automatically using zsh precmd-hook. Please add a line into your `.zshrc`:

```zsh
source <(llvmenv zsh)
```

If `$LLVMENV_RUST_BINDING` environmental value is non-zero, llvmenv exports `LLVM_SYS_60_PREFIX=$(llvmenv prefix)` in addition to `$PATH`.

```zsh
export LLVMENV_RUST_BINDING=1
source <(llvmenv zsh)
```

This is useful for [llvm-sys.rs](https://github.com/tari/llvm-sys.rs) users. Be sure that this env value will not be unset by llvmenv, only overwrite.

# Concepts

## entry

- **entry** describes how to compile LLVM/Clang
- Two types of entries
  - *Remote*: Download LLVM from Git/SVN repository or Tar archive, and then build
  - *Local*: Build locally cloned LLVM source
- See [the module document](https://docs.rs/llvmenv/*/llvmenv/entry/index.html) for detail

## build

- **build** is a directory where compiled executables (e.g. clang) and libraries are installed.
- They are compiled by `llvmenv build-entry`, and placed at `$XDG_DATA_HOME/llvmenv` (usually `$HOME/.local/share/llvmenv`).
- There is a special build, "system", which uses system's executables.

## global/local prefix

- `llvmenv prefix` returns the path of the current build (e.g. `$XDG_DATA_HOME/llvmenv/llvm-dev`, or `/usr` for system build).
- `llvmenv global [name]` sets default build, and `llvmenv local [name]` sets directory-local build by creating `.llvmenv` text file.
- You can confirm which `.llvmenv` sets the current prefix by `llvmenv prefix -v`.
