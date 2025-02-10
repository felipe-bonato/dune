# Dune

_Dune_ is a simple File Explorer TUI made in Rust with an easy to use interface and fully customizable.

## Installation

1. Download the [latest release](https://github.com/felipe-bonato/dune/releases/) from GitHub.
2. Add the config to your profile (`~/.bashrc` or `~/.profile` or `~/.zshrc` or ...):
```shell
# This is necessary for dune to be able to change to the path you navigated.
# When you quit, dune will save the directory into the `/tmp/dune-cd.txt` file, then `cd` there.
alias dune='dune;cd $(cat /tmp/dune-cd.txt)'

# Adds you installation path to the PATH var.
# You can change this to whatever you want.
export PATH="$PATH:~/bin"
```

## How to use

TODO: Tutorial

```monospaced
__________
|________| <- Header
|    | | | <- Files list
|    | | |
|____|_|_|
|________| <- State
|________| <- Prompt
```
