# Third-party notices: vendored Bicameral dashboard

The prebuilt dashboard in `dashboard/` (served by Bicameral Cloud and shipped in its image) is the Vite production build of `BicameralAI/bicameral-bot` `dashboard/` at Bot commit `4deb00470346a571d611ef6fe2d0a36cc9138e66` (`dashboard/version.json`: `bundle_sha256` `05d2608ed8a5f64cf4a0d0dbf7c2ddaec0e9f7714dfac834df35296ad9d0ed94`). The dashboard's own source is Bicameral's and is covered by this repository's `LICENSE` and `NOTICE`.

The bundle declares no runtime dependencies. The only third-party code compiled into `dashboard/assets/index-CpXKs813.js` (sha256 `458c4b42f915cfd667041d43924b9a62cc3a192024a2fd1ac862452624c31e61`) is listed below. `dashboard/assets/index-DM98lMXS.css`, `dashboard/index.html` and `dashboard/favicon.svg` contain no third-party material.

| Package | Version | License | Bundled part |
| --- | --- | --- | --- |
| `svelte` | 5.56.0 | MIT | Client runtime |
| `esm-env` | 1.2.2 | MIT | Environment flags used by the Svelte runtime |
| `vite` | 6.4.3 | MIT | Module preload polyfill injected at build time |

Versions and integrity hashes are those in Bot's `dashboard/package-lock.json` at the commit above.

## How this list was verified

At the Bot commit above, `npm ci` in `dashboard/`, then `vite build` with an added Rollup plugin that writes `Object.keys(chunk.modules)` (modules with rendered output) for each chunk. The build output was byte-identical to the vendored `dashboard/` files, and its only modules outside the dashboard's `src/` were from `svelte`, `esm-env` and `vite/modulepreload-polyfill`. Development-only dependencies (test, type-check and build tooling) are not distributed and are not listed.

## svelte 5.56.0

Copyright (c) 2016-2025 [Svelte Contributors](https://github.com/sveltejs/svelte/graphs/contributors)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## esm-env 1.2.2

Copyright 2022 Benjamin McCann

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## vite 6.4.3 (core license)

MIT License

Copyright (c) 2019-present, VoidZero Inc. and Vite contributors

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
