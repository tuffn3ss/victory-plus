# VIctory Plus

VIctory Plus is a build of [hyphennn's victory-fork](https://github.com/fxceless/victory-fork),
itself a fork of [VIctory](https://github.com/Marvisak/VIctory), tuned on an
AMD Radeon card and a 32 GB PC. It follows victory-fork's `main` and adds a
few fixes of its own, mostly for frame rate and for the freezes while moving.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Status (2026-11-25)

Tested on one PC: AMD Radeon RX 7800 XT 16 GB, AMD Ryzen 5 7600X, 32 GB RAM,
Windows 11 Pro. The Series X profile is used throughout.

| | On this PC |
|---|---|
| How far | Playable in the first gameplay scene (the prologue): the character moves, and the HUD, pause menu and pad input work. Not further into the story yet. |
| Picture | Lit, with textures, characters and sky. Anything the sun does not hit directly is black, because the game's ray-traced lighting is not emulated yet (same as victory-fork). |
| Frame rate | 6 to 8 fps in the first rooms (155 to 160 ms a frame), with the switches under "Running it". |
| Freezes while moving | Far fewer. Frames over one second fell from 47 to 11 and 25 in 5-minute runs (see below). |
| Stability | No crash in the recent 5-minute test runs. One earlier run lost the GPU device when Windows ran out of commit memory, so give Windows a large page file. |

## What VIctory Plus adds to victory-fork

- **Memory floor** (`VICTORY_MEMORY_FLOOR_GB`, off unless set). The freezes
  while moving came from Windows trimming the emulator's working set, about
  20 GB, when free RAM runs low. Every remap of the game's memory then waits
  on the page tables: 0.4 to 1 ms each instead of about 2 µs, and a frame
  that streams textures does hundreds of them. The switch asks Windows for a
  hard minimum working set of that many GB. If Windows refuses, the layer
  steps down 2 GB at a time to 4 GB and logs what it got. Measured over
  5-minute runs with scripted input:

  | | frames over 1 s | mean frame |
  |---|---|---|
  | before | 47 | 698 ms |
  | `VICTORY_MEMORY_FLOOR_GB=14`, two runs | 11 and 25 | 192 and 357 ms |

- **Texture staging from our own upload heaps** (on by default;
  `VICTORY_STAGING_ARENA=0` turns it off). Staging buffers are slots of
  64 MiB upload heaps and are not created and freed one by one. Upload
  buffers created in a five-minute run went from 23,877 (16.4 GB) to 2,701
  (312 MB). Frame time in steady scenes is unchanged (167.4 against 166.8
  ms); it saves work in streaming bursts.
- **Texture sizes read from heap descriptors.** Pixel programs that read a
  texture's width, height or first mip from its descriptor now translate.
  All 7 saved failing pixel shaders translate and validate, including the
  draw dropped most often in the prologue.
- **A switch set measured on AMD.** `VICTORY_COPY_CHANGED=1` takes the frame
  from 167 to 159 ms on the RX 7800 XT (on victory-fork's RTX 4070 it gave no
  net gain).
- victory-fork's new commits, merged as they land.

## Running it

Requirements are as upstream (below): Windows 10/11 x64, a CPU with AVX2, a
Direct3D 12 GPU with shader model 6.6 and mesh shaders, and Rust. The process
uses about 20 GB, so 32 GB of RAM and a large page file are needed.

```bat
cargo build --release -p victory
```

Then save this as a `.bat` file and run it:

```bat
@echo off
cd /d <the folder you cloned victory-plus into>
set "VICTORY_INDIRECT_WINDOW=1"
set "VICTORY_COPY_PLAIN=1"
set "VICTORY_JOIN_PARTS=1"
set "VICTORY_BACK_OWN_VIEW=1"
set "VICTORY_INDIRECT_GATHER=1"
set "VICTORY_CARD_REHASH_RUNS=1"
set "VICTORY_COPY_CHANGED=1"
set "VICTORY_TEXTURE_CATCHUP=0"
rem Keeps 14 GB of the emulator in RAM. On a PC with less than 32 GB, lower it.
set "VICTORY_MEMORY_FLOOR_GB=14"
target\release\victory.exe run "<your game folder>\<the title's exe>" --execute
pause
```

## Credits

- **[victory-fork](https://github.com/fxceless/victory-fork)** by hyphennn is
  nearly all of what runs here: its rendering and speed work. Its README has
  the full history of that work and every switch it adds.
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of both: the loader, the runtime services, the
  Direct3D 12 back end and the shader recompiler. Its README covers setup,
  commands and debugging.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors was a
  reference for how the title behaves under another layer.
- The guard over every view of a memory page is by **myokex**
  ([victory-amd](https://github.com/myokex/victory-amd)), building on work by
  Spexivus, joroyt and tightfall; the read-beside fix for kernel reads is by
  **Joshua Odom**.
- VIctory Plus's own changes were developed with
  [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
