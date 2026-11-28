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

## Status (2026-11-26, evening; zero-copy added 2026-11-27)

Tested on one PC: AMD Radeon RX 7800 XT 16 GB, AMD Ryzen 5 7600X, 32 GB RAM,
Windows 11 Pro. Measured in the boat scene the save loads into, Series X
profile, with the switches under "Running it".

| | On this PC |
|---|---|
| How far | Through the prologue and out of the prison into the open world. The character moves, and the HUD, pause menu and pad input work. |
| Frame rate | About 20 to 22 fps (45 ms a frame in a 12-minute run on 2026-11-26; 54 ms before the memory fixes), with parts of the scene maxing out at 25 fps. Without VIctory-7900XTX's renderer patches it was 14 fps (71 ms); on victory-fork 6 to 7 fps. The first runs of a new build are far slower and hitch while the driver makes pipelines. |
| Picture | Sharp at 1080p: the game renders at 2560x1440 and draws in 4K, shrunk to the window. Lit, with textures, characters and sky. A stand-in light fills the shade, since the game's ray-traced lighting is too slow to use here. |
| Memory | 29.0 GB of private memory in play and steady from minute 3 to minute 12 (before the leak fixes: 31.6 GB at 6 minutes and 33.7 GB at 15, still rising), about 21 GB of it in RAM, 13.3 GB on the card. As a Series S (`--console series-s`): 27.3 GB and 11.8 GB at the same speed, but rendered at 1280x720 and soft. Give Windows a page file of 48 GB or more. |
| Stability | No crash in the recent 5-minute test runs. |
| Known problem | In places (the swamp of the boat mission) trees drop out for a frame and come back while their shadows stay. Neither VIctory-7900XTX's fix (`VICTORY_PORT_CONSTANTS_ORDER`) nor copying the game's CPU culling depth back (`VICTORY_OCCLUSION_BACK=1`) stopped it there. |

## What VIctory Plus adds to VIctory-7900XTX

- **Memory floor** (`VICTORY_MEMORY_FLOOR_GB`, off unless set). The freezes
  while moving came from Windows trimming the emulator's working set when
  free RAM runs low. Every remap of the game's memory then waits on the page
  tables: 0.4 to 1 ms each instead of about 2 µs, and a frame that streams
  textures does hundreds of them. The switch asks Windows for a hard minimum
  working set of that many GB. If Windows refuses, the layer steps down 2 GB
  at a time to 4 GB and logs what it got. On victory-fork, frames over one
  second fell from 47 to 11 and 25 in 5-minute runs with it at 14.
- **Memory leaks fixed (2026-11-26).** Our own heap grew by about 150 MB a
  minute in play, 1.0 GB at the start and 3.3 GB after 15 minutes, and kept
  going. The game leaves stack garbage in the padding of the view
  descriptions it writes, and the table of placed-view shapes (kept for the
  run) counted each garbage variant as a new shape: 3.1 million after
  15 minutes. Shapes now leave out the description's filler and the view
  words their dimension does not use: about 50,000 after 7 minutes instead
  of 800,000. Also fixed: a set that kept every garbage variant of a texture
  description, each host list keeping room for its longest recording of
  indirect draws (200 MB), and 8 idle 64 MB texture staging blocks kept
  (now 2). A heap sampler finds the code that holds the most
  (`VICTORY_HEAP_SAMPLE=<KiB between samples>`, diagnostics builds).
- **Zero-copy reads of the game's memory (2026-11-27)**
  (`VICTORY_POOL_COMMIT=1` with `VICTORY_INPLACE=2`). The game's physical
  pages are committed up front, so that sections of them open as a card heap,
  and buffers and windows that lie in one run of a section, or in the game's
  private memory, are read by the card where they are instead of through
  copies. Boat scene, 6 minutes: 45 to 50 ms a frame instead of 57. Do not
  combine with `VICTORY_LATE_WAIT`. The 6.2 GB page pool itself is still
  refused as a heap (too big, it seems), so only about 0.4 GB is read in place.
- **Smaller copies of the game's memory** (`VICTORY_WINDOW_TAILS=1`,
  `VICTORY_WINDOW_KB=256`): the copies the card reads cover what is read, not
  whole 4 MB blocks. About 1 GB less memory, same speed.
- **Fewer new buffers in busy scenes**: list uploads come from a kept pool,
  more spare batches are kept. In the boat, the time spent waiting for new
  memory fell from 8.5 to 1.75 seconds a minute.
- **Texture detiling on the card** (`VICTORY_GPU_DETILE=1`, off), ported from
  victory-fork with one scratch buffer per list instead of one per texture.
  Byte-exact against the CPU on the software adapter, but the CPU's detiling
  costs only about 0.5 ms a frame here, so it stays off.
- **A status line every 30 seconds**: the card's memory and budget, and
  textures refused for lack of room on the card. It tells from a player's
  log whether a scene that went black or blurry had a full card.
- **The frame rate in the top left corner** of the picture, from
  victory-fork (`crates/victory-gpu/src/fpsbox.rs`; `VICTORY_FPS_COUNTER=0`
  hides it). F3 still shows VIctory-7900XTX's own panel over it.
- **Joined submissions off** (`VICTORY_JOIN_PARTS=1` in the `.bat` below).
  With VIctory-7900XTX's 8, the intro animation was shown out of order: in a
  fast capture of the screen, about one moving frame in four went back to
  the picture two frames before. With 1 it does not.
- **Keyboard and mouse laid out as in PC games**, with the mouse as the
  camera (below, under "Keyboard and mouse").
- **Belobug's AMD fixes** from
  [VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)
  (branch `rx9070xt`): the card's memory let go sooner when it fills,
  dynamic descriptor slots freed when a submission lands, and textures the
  game writes uploaded in order with the frame that uses them, against
  flicker and frames shown out of order.
- **VIctory-7900XTX's renderer patches**, textures that wait for their data
  looked at again every half second (`VICTORY_TEXTURE_LOOK_MS=500`; it was
  up to 13 to 18 seconds at this frame rate), and pipelines made before the
  run (`victory warm --used`, with `VICTORY_PIPELINE_BOOK=1` so runs write
  down what they make) instead of in the middle of play, all in the `.bat`
  below.
- **Measured and left out for the picture:** full 1080p as a Series S (the
  game stops itself at start), `VICTORY_OUTPUT=1920x1080` as a Series X
  (sharp, but the menus and captions stay laid out for 4K: twice too big
  and cut off), and the game's ray-traced light (`VICTORY_BVH_BUILDER=1`:
  102 ms a frame instead of 56, and 3 GB more memory).
- **A kernel fix** from upstream VIctory (Maraco91): a title image's file
  name that fits clears the last error.

Measured and left out: texture levels on demand (`VICTORY_LEVELS_ON_DEMAND=1`)
saved 3.9 GB of the card on victory-fork, but here it made about 11 GB of
upload buffers more and took the frame from 62 to 380 ms. It stays off, as in
VIctory-7900XTX.

Merged on 2026-11-26: VIctory-7900XTX up to 014d52d and the fixes on Belobug's
`a-publier` branch (an AMD shader compiler crash, the HUD's memory lines, warm
and profile fixes); their own keyboard and mouse mapping is left out for ours.

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
rem Crash reports are off (their bookkeeping took about 300 MB); to find why the card was lost: set "VICTORY_D3D12_DRED=light"
set "VICTORY_SKIP_PIPELINES=none"
set "VICTORY_THIN=1"
rem 8, as VIctory-7900XTX sets it, shows the intro animation out of order (it jumps back and forth).
set "VICTORY_JOIN_PARTS=1"
set "VICTORY_UPLOAD_AHEAD=1"
rem VIctory-7900XTX's renderer patches (off there by default).
set "VICTORY_PORT_CONSTANTS=1"
set "VICTORY_PORT_DESCRIPTORS=1"
set "VICTORY_PORT_CBV=1"
set "VICTORY_PORT_GUARD=1"
set "VICTORY_PORT_JOURNAL=1"
rem VIctory-7900XTX's tree flicker fix: about 4 ms a frame here, and it did not stop the flicker below.
set "VICTORY_PORT_CONSTANTS_ORDER=0"
set "VICTORY_TEXTURE_LOOK_MS=500"
rem 24 threads read the game's streamed data (default: one per CPU thread); detailed models load sooner.
set "VICTORY_DSTORAGE_WORKERS=24"
rem Keeps 14 GB of the emulator in RAM. On a PC with less than 32 GB, lower it.
set "VICTORY_MEMORY_FLOOR_GB=14"
rem Copies of the game's memory cut to what is read: about 1 GB less.
set "VICTORY_WINDOW_TAILS=1"
set "VICTORY_WINDOW_KB=256"
rem Zero-copy: the card reads parts of the game's memory in place (faster; not with VICTORY_LATE_WAIT).
set "VICTORY_POOL_COMMIT=1"
set "VICTORY_INPLACE=2"
rem Runs write down the pipelines they make; warm makes the new ones before the run.
set "VICTORY_PIPELINE_BOOK=1"
target\diag\victory.exe warm --used
rem Add --console series-s for 3.4 GB less memory and a softer picture.
target\diag\victory.exe run "<your game folder>\<the title's exe>" --execute
pause
```

### Keyboard and mouse

With no pad connected (or alongside one), the keyboard and mouse act as a
pad, laid out the way PC games usually are. Click in the picture to let the
mouse turn the camera. Hold Alt to get the pointer back (to move, resize or
maximize the window); it also comes back when you switch to another window.

| Key | Pad | On foot |
|---|---|---|
| W A S D | left stick | move |
| Mouse | right stick | camera, aim |
| Left button | right trigger | shoot |
| Right button | left trigger | aim |
| Shift | A | sprint |
| Space | X | jump |
| R | B | reload, melee |
| F | Y | get in or out of a car |
| Q | right shoulder | cover |
| Tab | left shoulder | weapon wheel |
| Ctrl | left stick click | crouch |
| C, middle button | right stick click | look behind |
| E | d-pad right | the prompt's action |
| Arrow keys | d-pad | phone and the rest |
| Esc | Menu | pause |
| V | View | camera mode |
| Enter, Backspace | A, B | menus |

**Caps Lock switches the driving keys** on and off, since a pad cannot
say whether you are in a car: W is the gas (right trigger), S the brake and
reverse (left trigger) and Space the handbrake (right shoulder). Press it
when you get in and again when you get out.

What each pad button does is the game's choice; the "On foot" column is the
series' usual pad layout and may differ in places. `VICTORY_MOUSE_SENS`
(1 by default) sets how fast the mouse turns the camera,
`VICTORY_MOUSE_INVERT=1` turns up and down around, and `VICTORY_MOUSE=0`
leaves the mouse alone.

`scripts\play.ps1` from VIctory-7900XTX sets the same switches but the
memory floor, and joins submissions (`VICTORY_JOIN_PARTS=8`).

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
- **[VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**
  by Belobug: the AMD fixes above, taken as his two commits.
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
