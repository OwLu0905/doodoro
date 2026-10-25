# Doodoro (GPUI)

A port of Doodoro, the hand-drawn pomodoro timer, from gpuix-svelte straight to
[GPUI](https://crates.io/crates/gpui) in Rust, made to compare against the gpuix-svelte and Tauri
versions (`../gpuix-podo`, `../doodoro-tauri`).

It keeps the gpuix-svelte version as close as it can:

| gpuix-svelte | GPUI |
|---|---|
| `App.svelte` | `src/main.rs`, one GPUI view |
| `wobble.ts` | `src/wobble.rs`, the same noise, so the outlines come out the same |
| `<svg source>` layers (`Shape.svelte`), icon SVGs | GPUI paths (`src/draw.rs`), nothing rasterised |
| `motion` prop | GPUI's `with_animation`, with the design's cubic-bézier easings |
| fonts installed for the user | compiled into the binary (only the three weights used) |
| `window-shim.m` loaded over FFI | the same shim, compiled by `build.rs` and linked in |
| opens hidden, moved into place, then shown | opens where it was last dropped: GPUI takes the position up front |

Settings go to `~/Library/Application Support/local.doodoro.gpui/settings.json` (or
`DOODORO_SETTINGS`), and the window's spot to the `local.doodoro.gpui.window` defaults domain,
apart from the other versions'.

## Running it

You need Rust and the Xcode command line tools. The first build compiles GPUI and takes a few
minutes.

```bash
cargo run --release
./bundle.sh                          # dist/Doodoro GPUI.app
DOODORO_SCREENSHOT=/tmp/app.png cargo run   # writes what the window draws, a second after it opens
```

## Benchmark

`node bench/bench.mjs` runs `../doodoro-tauri/bench` with this app added; it needs all three
built (`./bundle.sh` here, `npm run bundle` in the other two). On an Apple Silicon Mac,
2026-10-26:

| | GPUI | gpuix-svelte | Tauri |
|---|---|---|---|
| .app size | 10.2 MB | 94.2 MB | 9.2 MB |
| zipped | 5.8 MB | 38.0 MB | 7.4 MB |
| window on screen (median of 10) | 158 ms | 806.5 ms | 266.5 ms |
| processes | 1 | 1 | 4 |
| memory (footprint) | 37.7 MB | 61.8 MB | 109.9 MB |
| CPU, idle | 0.63% | 2.73% | 0.10% |

7.6 MB of the GPUI binary is fonts; the code is under 3 MB. Its idle CPU is GPUI's display link,
which wakes on every screen refresh whether or not anything changed. For Tauri the window can be
on screen before the WebView has painted the card, so its startup undercounts.

## License

The code is under the [MIT License](LICENSE). The fonts in `fonts/` are Zen Maru Gothic and
Grandstander, under the SIL Open Font License 1.1; the license texts sit next to them.
