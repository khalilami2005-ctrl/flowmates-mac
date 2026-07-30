# Third-party notices

Flowmates bundles the following third-party components. These notices
apply to those components only and do not replace Flowmates's own license.

## Runtime dependencies

### llama.cpp

The bundled `llama-server` binary is built from `ggml-org/llama.cpp` tag
`b10103` and is distributed under the MIT License.

Source: https://github.com/ggml-org/llama.cpp/tree/b10103

MIT License

Copyright (c) 2023-2026 The ggml authors

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

### Qwen3-VL

The bundled GGUF model and multimodal projector are derived from
`Qwen3-VL-2B-Instruct`, published by the Qwen team, Alibaba Cloud, under the
Apache License 2.0.

Source: https://github.com/QwenLM/Qwen3-VL

Apache License 2.0 — full text follows this page in the bundle.

### Tauri

The application framework is built on Tauri 2, licensed under Apache 2.0 OR MIT.

Source: https://github.com/tauri-apps/tauri

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## JavaScript dependencies (via npm/pnpm)

The following packages are bundled in the renderer bundle:

| Package | License |
|---|---|
| `@tauri-apps/api` | Apache-2.0 OR MIT |
| `@tauri-apps/plugin-process` | MIT OR Apache-2.0 |
| `@tauri-apps/plugin-updater` | MIT OR Apache-2.0 |
| `dompurify` | MPL-2.0 OR Apache-2.0 |
| `jspdf` | MIT |
| `html2canvas` | MIT |
| `canvg` | MIT |
| `core-js` | MIT |
| `pako` | MIT AND Zlib |
| `base64-arraybuffer` | MIT |
| `css-line-break` | MIT |
| `fast-png` | MIT |
| `fflate` | MIT |
| `iobuffer` | MIT |
| `performance-now` | MIT |
| `raf` | MIT |
| `regenerator-runtime` | MIT |
| `rgbcolor` | MIT |
| `stackblur-canvas` | MIT |
| `svg-pathdata` | MIT |
| `text-segmentation` | MIT |
| `utrie` | MIT |

## Rust dependencies (via cargo)

The Rust backend uses approximately 670 crates. The majority are licensed under
MIT, Apache-2.0, or both. The full inventory can be regenerated with:

```bash
cd apps/agent/src-tauri
cargo install cargo-about --features cli
cargo about init          # creates about.hbs + about.toml
# Edit about.toml to add allowed licenses (see below)
cargo about generate about.hbs > about.html
```

### Allowed licenses (pre-release checklist)

Before distribution, edit `apps/agent/src-tauri/about.toml` to accept:

- MIT
- Apache-2.0
- MIT OR Apache-2.0 (and similar dual-license variants)
- BSD-2-Clause, BSD-3-Clause
- ISC
- Zlib
- MPL-2.0
- Unicode-3.0
- CC0-1.0
- Unlicense
- CDLA-Permissive-2.0
- Apache-2.0 WITH LLVM-exception
- NCSA

License counts by category (current, 672 packages):

| Count | License expression |
|---|---:|
| 304 | MIT OR Apache-2.0 |
| 159 | MIT |
| 39 | Apache-2.0 OR MIT |
| 37 | MIT/Apache-2.0 |
| 18 | Zlib OR Apache-2.0 OR MIT |
| 18 | Unicode-3.0 |
| 17 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| 8 | BSD-3-Clause |
| 7 | Apache-2.0/MIT |
| 7 | Apache-2.0 OR ISC OR MIT |
| 6 | MPL-2.0 |
| 5 | Unlicense OR MIT |
| 5 | ISC |
| 3 | BSD-2-Clause |
| 2 | Zlib |
| rest | various permissive dual-license |

## License texts

### Apache License, Version 2.0

The Apache 2.0 text is reproduced below.

### MIT License

MIT License is reproduced as part of the llama.cpp section above.

### MPL-2.0

Mozilla Public License 2.0 — used by `dompurify` and several Rust crates
(`cssparser`, `selectors`, `webpki-roots`).

### Unicode-3.0

Unicode License v3 — used by ICU4X crates (`icu_*`, `zerovec`, `writeable`,
`yoke`, `zerofrom`, `litemap`, `tinystr`, `zerotrie`, `potential_utf`).

See https://www.unicode.org/license.html

### CDLA-Permissive-2.0

Community Data License Agreement — Permissive 2.0 — used by
`webpki-root-certs`.

See https://cdla.dev/permissive-2-0/

---

No license is published for Flowmates's own code at this time; copyright applies
alone, all rights reserved. The notices above are obligations owed to the
third-party components listed, and they stand regardless of that choice.
