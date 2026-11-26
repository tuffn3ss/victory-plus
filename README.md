# VIctory Plus

VIctory Plus is optimized with the engine code of
[avatarmanz's VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX),
which grew out of [hyphennn's victory-fork](https://github.com/fxceless/victory-fork)
and [VIctory](https://github.com/Marvisak/VIctory). On top of it, Plus adds a
memory floor against freezes while moving, a status line in the log and an
upstream kernel fix, with measurements from an AMD Radeon RX 7800 XT with
16 GB on a 32 GB PC.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Status (2026-11-25)

Tested on one PC: AMD Radeon RX 7800 XT 16 GB, AMD Ryzen 5 7600X, 32 GB RAM,
Windows 11 Pro. The Series X profile is used throughout.

| | On this PC |
|---|---|
| How far | Through the prologue and out of the prison into the open world. The character moves, and the HUD, pause menu and pad input work. |
| Frame rate | 16 fps in the prison (62 ms a frame) with the switches under "Running it", against 6 to 7 fps (145 ms) on victory-fork. The first run of a new build is far slower while the driver makes pipelines; from the second run on it is not. |
| Picture | Lit, with textures, characters and sky. A stand-in light fills the shade, since the game's ray-traced lighting is not emulated yet. |
| Memory | About 36 GB of private memory, 14 GB of it in RAM: give Windows a page file of 48 GB or more. The card held 13.4 of its 14.5 GB budget in the prison. |
| Stability | No crash in the recent 5-minute test runs. |

## What VIctory Plus adds to VIctory-7900XTX

- **Memory floor** (`VICTORY_MEMORY_FLOOR_GB`, off unless set). The freezes
  while moving came from Windows trimming the emulator's working set when
  free RAM runs low. Every remap of the game's memory then waits on the page
  tables: 0.4 to 1 ms each instead of about 2 µs, and a frame that streams
  textures does hundreds of them. The switch asks Windows for a hard minimum
  working set of that many GB. If Windows refuses, the layer steps down 2 GB
  at a time to 4 GB and logs what it got. On victory-fork, frames over one
  second fell from 47 to 11 and 25 in 5-minute runs with it at 14.
- **A status line every 30 seconds**: the card's memory and budget, and
  textures refused for lack of room on the card. It tells from a player's
  log whether a scene that went black or blurry had a full card.
- **A kernel fix** from upstream VIctory (Maraco91): a title image's file
  name that fits clears the last error.

Measured and left out: texture levels on demand (`VICTORY_LEVELS_ON_DEMAND=1`)
saved 3.9 GB of the card on victory-fork, but here it made about 11 GB of
upload buffers more and took the frame from 62 to 380 ms. It stays off, as in
VIctory-7900XTX.

What VIctory-7900XTX itself changed, and why, is in
[docs/README-avatarmanz.md](docs/README-avatarmanz.md) and
[docs/findings.md](docs/findings.md); victory-fork's own README is in
[docs/README-fxceless.md](docs/README-fxceless.md).

## Running it

Requirements are as upstream: Windows 10/11 x64, a CPU with AVX2, a Direct3D
12 GPU with shader model 6.6 and mesh shaders, Rust, and the MSVC build tools.

```bat
cargo build --locked --profile diag -p victory --features diagnostics
```

Then save this as a `.bat` file and run it:

```bat
@echo off
cd /d <the folder you cloned victory-plus into>
set "VICTORY_TEXTURE_NO_WAIT=1"
set "VICTORY_RELAY=2"
set "VICTORY_WIDE_BACK=1"
set "VICTORY_CARD_REHASH_RUNS=1"
set "VICTORY_JOIN_COPIES=1"
set "VICTORY_INDIRECT_WINDOW=1"
set "VICTORY_RELAY_COPY=1"
set "VICTORY_CARD_STEPS=2"
set "VICTORY_INDIRECT_BATCH=2"
set "VICTORY_MAP_THREADS=1"
set "VICTORY_TITLE_PRIORITIES=0"
set "VICTORY_CARD_AWAKE=1"
set "VICTORY_SHOW_AFTER=1"
set "VICTORY_AMBIENT=0.8"
set "VICTORY_D3D12_DRED=light"
set "VICTORY_SKIP_PIPELINES=none"
set "VICTORY_THIN=1"
set "VICTORY_JOIN_PARTS=8"
set "VICTORY_UPLOAD_AHEAD=1"
rem Keeps 14 GB of the emulator in RAM. On a PC with less than 32 GB, lower it.
set "VICTORY_MEMORY_FLOOR_GB=14"
target\diag\victory.exe run "<your game folder>\<the title's exe>" --execute
pause
```

`scripts\play.ps1` from VIctory-7900XTX sets the same switches but the
memory floor.

## Credits

- **[VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** by
  avatarmanz is nearly all of what runs here: the list calls translated on
  threads of their own, submissions prepared on a thread of their own, the
  textures' own copy queue, and the rest of its speed work.
- **[victory-fork](https://github.com/fxceless/victory-fork)** by hyphennn is
  what VIctory-7900XTX and earlier VIctory Plus started from.
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of all of them: the loader, the runtime services,
  the Direct3D 12 back end and the shader recompiler.
- VIctory-7900XTX also carries work from
  [DexNom](https://github.com/DexNom/VIctory),
  [Belobug](https://github.com/Belobug/VIctory-Belobug-AMD) and romainhedouin,
  [ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY),
  [myokex](https://github.com/myokex/victory-amd) with Spexivus, joroyt and
  tightfall, Joshua Odom, Geckomacabre and cxy0072019-sketch; each commit taken
  names its source.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors was a
  reference for how the title behaves under another layer.
- VIctory Plus's own changes were developed with
  [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE); the one file that is not covered is
named in [docs/asset-policy.md](docs/asset-policy.md).
