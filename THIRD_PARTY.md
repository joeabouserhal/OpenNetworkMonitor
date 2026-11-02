# Third-party credits

The dependency list and exact resolved versions are in Cargo.toml and Cargo.lock. Dependencies retain their respective licenses; this file is not a complete redistribution license bundle.

- Other direct dependencies, all MIT or Apache-2.0: [anyhow](https://github.com/dtolnay/anyhow), [chrono](https://github.com/chronotope/chrono), [csv](https://github.com/BurntSushi/rust-csv), [serde](https://github.com/serde-rs/serde) and serde_json, [windows](https://github.com/microsoft/windows-rs) and windows-sys (Microsoft), raw-window-handle, and, for tests only, [tempfile](https://github.com/Stebalean/tempfile).
- Fonts: the interface is set in [Inter](https://github.com/rsms/inter) (Regular, Medium and SemiBold, version 4.1), embedded in the executable under the SIL Open Font License 1.1; its notice is in assets/fonts/Inter-LICENSE.txt. Cascadia Mono is used for code-like text when installed on the user's PC and is not bundled. egui's bundled fallback fonts are Ubuntu-Light, Hack, NotoEmoji-Regular and emoji-icon-font. Ubuntu-Light is under the [Ubuntu Font Licence](https://ubuntu.com/legal/font-licence), Hack under the MIT licence with the Bitstream Vera licence for its base glyphs, and NotoEmoji under the SIL Open Font Licence 1.1. Their full notices are published with egui at https://github.com/emilk/egui/tree/master/crates/epaint_default_fonts.
- Build-time resource tools: [embed-resource](https://github.com/nabijaczleweli/rust-embed-resource) and [ico](https://github.com/mdsteele/rust-ico) (MIT).
- egui / eframe: https://github.com/emilk/egui (MIT OR Apache-2.0).
- FerrisETW: https://github.com/n4r1b/ferrisetw (MIT). The project credits Microsoft's KrabsETW.
- tray-icon / muda / rfd: https://github.com/tauri-apps (MIT OR Apache-2.0, see individual crates).
- rusqlite: https://github.com/rusqlite/rusqlite (MIT); bundled SQLite is public domain.
- maxminddb reader: https://github.com/oschwald/maxminddb-rust (ISC). No GeoIP/GeoLite database is bundled. Users supply a database under its provider's terms.

The Nord and Catppuccin settings apply selected palette colors to native egui visuals; they are not third-party theme engines. The application's Slate and Light presets are original combinations with egui defaults.

## Palette license notice

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
