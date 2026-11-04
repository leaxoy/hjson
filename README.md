# HJson

[中文](README.zh-CN.md)

HJson is a native JSON viewer and structure comparer built with Rust, GPUI, and
gpui-kit. It is designed for quickly opening JSON from a file, clipboard,
manual input, URL, or session history, then inspecting it as both formatted text
and a navigable tree.

All parsing, formatting, comparison, and history state stay local to the app.

## Features

- Open JSON from clipboard, files, URLs, manual input, sample data, or prior
  session imports.
- View a document as formatted text, a structural tree, or both side by side.
- Switch the document view between split, text-only, and tree-only modes from
  the pane toolbar.
- Filter the tree by keys or values, expand/collapse nodes, and keep tree
  selection linked to the raw text line.
- Copy keys, scalar values, subtrees, JSONPath / JSON Pointer / jq paths, or the
  full document from the tree context menu.
- Open any tree subtree in a new tab, including JSON strings that can be parsed
  into their own document.
- Show timestamp hints for time-like fields such as `created_time`,
  `updated_at`, `epoch`, and `timestamp`.
- Compare two JSON documents by JSONPath, ignoring object key order and aligning
  arrays by index.
- Tune language, appearance, JSON font, wrapping, line numbers, key sorting,
  and syntax themes in settings.
- Ship installable artifacts for macOS, Linux, and Windows from tagged releases.

## Keyboard Shortcuts

On non-macOS platforms, use `Ctrl` where this table says `Cmd`.

| Shortcut | Action |
| --- | --- |
| `Cmd+T` | New import tab |
| `Cmd+D` | New comparison tab |
| `Cmd+O` | Open file |
| `Cmd+Shift+V` | Paste JSON from clipboard |
| `Cmd+W` | Close tab |
| `Cmd+Shift+H` | Toggle history sidebar |
| `Cmd+1` ... `Cmd+9` | Select tab |
| `Cmd+F` | Focus the document tree filter |
| `Cmd+Shift+E` | Expand the tree |
| `Cmd+Shift+C` | Collapse the tree |
| `Cmd+[` / `Cmd+]` | Move between diff changes |
| `Cmd+=` / `Cmd+-` / `Cmd+0` | Zoom in, zoom out, reset zoom |
| `Cmd+Ctrl+F` | Toggle fullscreen on macOS |
| `F11` | Toggle fullscreen on non-macOS platforms |

Within a document, `Tab` moves focus between text and tree panes. In the tree,
use arrow keys and `Enter` to navigate and expand nodes. In the raw text pane,
use arrow, page, home, and end keys to scroll.

## Development

Prerequisites:

- Rust stable toolchain.
- Platform native build dependencies required by GPUI.

Run the app:

```sh
cargo run
```

Check the project:

```sh
cargo fmt --all -- --check
cargo check
cargo test
git diff --check
```

Build a release binary:

```sh
cargo build --release
```

## Releases

Pushing a `v*` tag runs the GitHub Actions release workflow and publishes
installers or archives for:

- macOS arm64 and x86_64
- Linux arm64 and x86_64
- Windows arm64 and x86_64

Manual workflow runs build and upload artifacts only; they do not publish a
GitHub release.
