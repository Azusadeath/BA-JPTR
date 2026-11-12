# Third-party notices

The application source is licensed under GPL-3.0-only. The full license is in `LICENSE`.

## Origin of the text-field patcher

This project was substantially inspired by [Agent-0808](https://github.com/Agent-0808)
and their [BA-Term-Replacer (BATR)](https://github.com/Agent-0808/BA-Term-Replacer).
The public explanation and implementation informed the JP resource adaptation and desktop workflow.

The FlatBuffer string parsing and append-and-redirect patching implementation was ported from
[Agent-0808/BA-Term-Replacer](https://github.com/Agent-0808/BA-Term-Replacer), GPL-3.0.
That project credits [alisns1/blue-archive-gl-simplified](https://github.com/alisns1/blue-archive-gl-simplified)
and its contributors. The original MIT notice is retained in `licenses/blue-archive-gl-simplified-MIT.txt`.

The application does not include BATR's OpenCC dictionaries, automatic translation-name downloads,
Python runtime, or CRC-preserving installation implementation.

## Rust dependencies and SQLite

The resolved dependency inventory and available license texts are included under
`licenses/rust-dependencies/`. They cover the compiled Windows dependency graph,
including egui/eframe and its bundled fonts, the RustCrypto libraries, rusqlite and bundled SQLite.
SQLite is in the public domain; see https://www.sqlite.org/copyright.html.
Windows system libraries and the installed graphics driver are supplied by the operating system.

The portable build imports Windows system DLLs only; GCC/pthread runtime DLLs are not distributed.
The source archive, Cargo.lock and build/package scripts accompany this release.
