# For LLMs

Kael publishes an [`llms.txt`](https://augani.github.io/kael/llms.txt) file at
the site root. It gives an assistant the current architecture, platform limits,
browser build contract, and the right source files to inspect.

Use it as context when an assistant writes or reviews a Kael application:

```text
https://augani.github.io/kael/llms.txt
```

Every guide page also has a **Copy page** action in the top bar. It copies clean
Markdown for a focused question. Use `llms.txt` for framework wide context and
Copy page for the guide you are working from.

Useful entry points:

* [Object guide](object-guide.md)
* [One codebase](one-codebase.md)
* [Browser and WebAssembly](browser.md)
* [Web build and deployment](web-deployment.md)
* [What remains](remaining-work.md)
