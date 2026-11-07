# Jaywalk

A native Rust/GPUI Jujutsu comparison viewer. The prototype uses GPUI Kit's
existing text and panel components. Plans and research live in Homelab in the
Obsidian vault.

## Build

Requires Rust, jj 0.47+, and full Xcode on macOS. Build without changing the
global developer directory:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build
cargo run -- /path/to/jj/repository
```

GPUI Kit is pinned to 0.8.0: its 0.9.0 macOS backend currently uses an unstable
Rust API that fails on the installed stable compiler. The lockfile records the
verified dependency set. No framework patches are required.

## Development history

`main` is the minimal Rust baseline. The mutable `viewer-prototype` bookmark
holds the evolving native implementation. Use jj for history operations.
