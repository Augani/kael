# Web build and deployment

`kael web build` creates a static site. A server runtime is not required.

## Build

Install the exact tools expected by Kael 0.4:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
```

Then build:

```sh
kael web build
```

Release builds run `wasm-opt -O3`. Use `kael web build --debug` for a faster
local build without release optimization.

Useful options:

| Option | Purpose |
|---|---|
| `--out-dir <path>` | Change `dist/web` |
| `--html <file>` | Package a source-owned host page |
| `--assets <directory>` | Copy product web assets into the output |
| `--package <name>` | Select a workspace package |
| `--bin <name>` | Select a binary target |
| `--port <number>` | Change the development server port |
| `--no-open` | Serve without opening a browser |

The last two options apply only to `kael web serve`.

## Deploy

Upload the full output directory and preserve its relative paths:

```text
dist/web/
├── index.html
├── app.js
└── app_bg.wasm
```

The host must:

* serve `app_bg.wasm` as `application/wasm`
* allow JavaScript modules
* serve all three files from the same origin by default
* use HTTPS for permission based browser APIs; localhost is valid for development

Deploy the three files together. Their names are stable, not content hashed.
Do not apply immutable caching unless your own build step fingerprints them.
Revalidate the HTML, JavaScript, and Wasm files after a release.

If the app uses history based routes, configure the host to fall back to
`index.html`.

## Use a custom host page

Keep a custom host page in the source tree and pass it to either build command:

```sh
kael web build --html web/index.html --assets web/assets
```

The HTML file becomes `dist/web/index.html`. Asset directory contents keep
their relative paths. Kael rejects asset symlinks and reserved root files named
`index.html`, `app.js`, or `app_bg.wasm`, so product assets cannot silently
replace the generated application.

The host page must contain a canvas with the expected id and load the generated
module:

```html
<canvas id="blade" aria-label="Kael application"></canvas>
<script type="module">
  import init from "./app.js";
  await init({ module_or_path: "./app_bg.wasm" });
</script>
```

The default page contains inline style and module code. A strict Content
Security Policy needs a custom page with external files, a nonce, or matching
hashes.

The CLI does not fingerprint deployment assets or generate a service worker.
Add those product policies after `kael web build` when required.

## Verify the deployment

Open the deployed page and confirm:

1. `app.js` loads as a JavaScript module.
2. `app_bg.wasm` returns `application/wasm`.
3. The browser creates a WebGL2 context.
4. Pointer, keyboard, text input, and scrolling reach the application.
5. Permission based workflows run from a user action.

No product JavaScript is needed for normal Kael interactivity. See
[Browser and WebAssembly](browser.md) for the runtime contract and limits.
