# Pawn Demo

A small Rust web server that serves a single page application. The page shows
a black 3D chess pawn standing on a tilted 5×5 board, and you move the pawn
one square at a time.

- **Random port.** The server picks a random port from 1001 to 65535 at
  startup and prints it to the terminal.
- **In-memory store.** There is no database. Each visitor's data is a
  `UserData` struct in a `HashMap` in memory. It is lost when the server stops,
  and sessions idle for more than an hour are removed.
- **64-bit session cookie.** On a visitor's first request the server sets a
  `pawn_sid` cookie (`HttpOnly`, `SameSite=Strict`). Its value is 64 bits from
  the operating system's cryptographic random number generator, shown as 16
  hex digits. The cookie is the key to that visitor's `UserData`.
- **AJAX, no WebSockets.** The page sends plain `fetch` requests. The server
  checks every move and returns the new state.
- **No external front-end libraries.** The 3D scene is drawn on a `<canvas>`
  by a small built-in renderer. The pawn is a solid of revolution with glossy
  shading. The page is compiled into the binary, so the program is a single
  self-contained executable.

## Project layout

```
Cargo.toml
src/main.rs         server, session store, API
static/index.html   single page application (embedded via include_str!)
```

## API

| Method | Path         | Body             | Response                                   |
|--------|--------------|------------------|--------------------------------------------|
| GET    | `/`          |                  | the single page application                |
| GET    | `/api/state` |                  | `{"x":2,"y":4,"moves":0,"size":5}`         |
| POST   | `/api/move`  | `{"x":3,"y":3}`  | new state, or `422` with `{error, state}`  |
| POST   | `/api/reset` |                  | the starting state                         |

`x` is the column (0–4, left to right) and `y` is the row (0 is the far,
uphill edge). A move is valid if the target is one square away in any
direction, diagonals included.

---

## Linux

These steps assume the Rust toolchain (`cargo` and `rustc`, 1.85 or newer for
edition 2024) is already installed.

### Build

```bash
git clone <this repository> pawn-demo
cd pawn-demo
cargo build --release
```

The binary is written to `target/release/pawn_demo`.

### Run

```bash
./target/release/pawn_demo
# or build and run in one step:
cargo run --release
```

The terminal shows the port:

```
Pawn demo listening on port 24180
Open http://127.0.0.1:24180/ in your browser (Ctrl+C to stop)
```

### Access

Open the printed URL in any modern browser (Firefox, Chrome, and so on), for
example `http://127.0.0.1:24180/`. The port is different each time the server
starts. Press `Ctrl+C` in the terminal to stop the server.

---

## Windows

### Install the toolchain (one time)

1. Install the **Visual Studio Build Tools** with the *Desktop development
   with C++* workload. The Rust MSVC toolchain needs its linker. From an
   administrator PowerShell:

   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

   You can also download the installer from
   <https://visualstudio.microsoft.com/visual-cpp-build-tools/>.
2. Install Rust with **rustup**:

   ```powershell
   winget install Rustlang.Rustup
   ```

   You can also download and run `rustup-init.exe` from <https://rustup.rs>
   and accept the defaults.
3. Open a **new** PowerShell or Command Prompt window so that `cargo` is on
   your `PATH`, then check it:

   ```powershell
   cargo --version
   ```

### Build

```powershell
git clone <this repository> pawn-demo
cd pawn-demo
cargo build --release
```

If you don't have Git, download the repository as a ZIP, extract it, and `cd`
into the extracted folder.

The binary is written to `target\release\pawn_demo.exe`.

### Run

```powershell
.\target\release\pawn_demo.exe
# or build and run in one step:
cargo run --release
```

The console shows the port and URL, for example:

```
Pawn demo listening on port 24180
Open http://127.0.0.1:24180/ in your browser (Ctrl+C to stop)
```

### Access

Open the printed URL in Edge, Chrome, or Firefox. The server listens on
localhost only by default, so Windows Firewall does not prompt. Press `Ctrl+C`
in the console window to stop the server.

---

## Controls

- **Mouse:** click one of the highlighted squares next to the pawn.
- **Keyboard:** `↑ ↓ ← →` or `W A S D` move one square; `Q E Z C` move
  diagonally.
- **Reset:** puts the pawn back on its starting square and sets the move
  count to zero.

## Options

The server binds to `127.0.0.1` by default. To reach it from other machines on
your network, set `PAWN_BIND`:

```bash
PAWN_BIND=0.0.0.0 ./target/release/pawn_demo          # Linux
```

```powershell
$env:PAWN_BIND = "0.0.0.0"; .\target\release\pawn_demo.exe   # Windows PowerShell
```

Then browse to `http://<server-ip>:<port>/`. On Windows, allow the program
through the firewall when prompted. This is an alpha demo with no TLS, so only
expose it on networks you trust.
