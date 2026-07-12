# Glyph-80

Glyph-80 is a from-scratch text-display system — hardware, firmware, and software — built up in phases from a blinking LED matrix to a microcode-driven VGA terminal. Along the way it grows the tools it needs, starting with **FontSpace**, a bitmap-font creation tool.

The name is for the 80-column text display the later phases target.

## Repository layout

The project is organized as a set of sub-projects, one per directory. Each sub-project owns its own build, its own docs, and its own `CLAUDE.md`.

| Directory | What it is | Status |
| --- | --- | --- |
| [FontSpace/](FontSpace/) | Bitmap-font creation tool (Rust). Design and edit the glyph bitmaps the hardware renders. | First up |
| _(later phases)_ | Hardware / firmware / display sub-projects — see roadmap below. | Planned |

## Roadmap

The project is imagined as a sequence of increasingly capable steps. Each builds on the last:

1. **FontSpace.** A bitmap-font creation tool (Rust). It produces the glyph bitmaps every later step displays — so it comes first.
2. **Glyph cycler.** A breadboard system with an LED matrix display (targeting 8×16) that cycles through all the glyphs in a font EEPROM.
3. **VGA text display.** A breadboard system with a font ROM and a display ROM/RAM of (initially) 80×30 characters, driving glyph display out a VGA port — similar in spirit to the Apple I/II text display.
4. **Serial terminal.** As above, but fed by a serial byte interface (UART/USB) streaming display bytes into the display RAM. Handles newline, line wrap, and scroll-up as new lines arrive at the bottom.
5. **Advanced terminal.** As above, plus its own control commands — each introduced by an `ESC` code with _n_ parameter bytes, implemented in microcode: move cursor to coordinates, clear screen, switch font page, change color, styles, etc.

_How far this gets is an open question — that's part of the fun._

## Getting started

Each sub-project documents its own build and test workflow in its directory. Start with [FontSpace/](FontSpace/).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
