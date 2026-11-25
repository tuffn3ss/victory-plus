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
| How far | Through the prologue and out of the prison into the open world (the chase after it, at about 4 fps). The character moves, and the HUD, pause menu and pad input work. Outside, the card's memory used to fill, so new textures waited and parts of the scene came out black or blurry. Texture levels on demand (below) leave about 4 GB more room. This is not yet tested outside. |
| Picture | Lit, with textures, characters and sky, and indoor rooms and shade no longer black: a constant ambient light stands in for the game's ray-traced lighting, which is not emulated yet. Some faces turned away from lamps are still dark. |
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

- **Texture levels on demand, on by default** (`VICTORY_LEVELS_ON_DEMAND=0`
  turns it off; `VICTORY_LEVELS_MIN_KB`, 256 by default, sets the smallest
  texture it covers). victory-fork's switch for this is off by default and
  covers textures from 1 MB. The game streams texture detail in and holds
  only some levels of most textures, but each host copy took card memory
  for all of them. Now a texture gets memory only for the levels the game
  holds. In two 5-minute runs in the prison on the RX 7800 XT, the card
  held 8.9 GB with it and 12.8 GB without it (5.8 GB of levels left
  unallocated). Texture passes that came up empty for lack of room fell
  from 1,618 to none. Frame time was the same, about 145 ms. This is the
  room the open world needs: before, it filled the card, and new textures
  came out black or blurry.
- **Ambient light in the shade** (on by default; `VICTORY_AMBIENT=off`
  turns it off, `VICTORY_AMBIENT=<level>` sets it, 0.6 by default). Ported
  from [Belobug's VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD).
  The game's indirect light comes from ray tracing, which does not run yet,
  so everything the sun did not reach was black. Right after the game
  resolves that light, it is replaced by a constant, with less of it in
  creases and under objects, and glossy surfaces no longer reflect the open
  sky indoors. No frame-rate cost measured: 140 to 160 ms a frame in the
  cell. With `VICTORY_BVH_BUILDER=1` it stays off unless a level is set.
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
- **Upstream VIctory's audio fixes** (eight commits by AnoHunted, Maraco91,
  tedin7, TG0000, heromike405-arch and divyeshganta), which victory-fork does
  not have yet. Among them: an XMA stream that loops with its read block past
  the end of its ring no longer keeps its voice, and the audio lock, filling
  forever, which silenced all sound.
- **A status line every 30 seconds in every build**: the card's memory and
  budget, textures refused for lack of room on the card, the ambient
  light's passes, and the idle texture copies that only a descriptor slot
  keeps on the card (about 2.3 GB in the prison). It tells from a player's log whether a scene that went
  black or blurry had a full card.
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
- **[VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)** by
  Belobug wrote the ambient light in `crates/victory-gpu/src/ambient.rs`,
  ported here without its unfinished sky-following part.
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
