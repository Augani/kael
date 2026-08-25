<div class="kael-home">
  <section class="home-hero" aria-labelledby="kael-title">
    <p class="eyebrow">Kael 0.4 · Rust application framework</p>
    <h1 id="kael-title">One codebase.<br>Every serious surface.</h1>
    <p class="hero-lede">Kael is a retained, GPU-accelerated framework for building ambitious applications in Rust. Design the interface once, keep the product logic once, and run it as a native desktop app or a WebAssembly application in the browser.</p>
    <div class="hero-actions">
      <a class="button-primary" href="getting-started.html">Start building</a>
      <a class="button-secondary" href="framework-today.html">Explore the framework <span aria-hidden="true">→</span></a>
      <a class="button-secondary" href="https://github.com/Augani/kael" target="_blank" rel="noopener noreferrer">View on GitHub <span aria-hidden="true">↗</span></a>
    </div>
    <p class="platform-line" aria-label="Supported targets"><span>macOS</span><span>Windows</span><span>Linux</span><span>Browser</span></p>
  </section>

  <section class="architecture-figure" aria-label="One application architecture targeting four renderers">
    <div class="architecture-source">
      <p class="figure-label">Your application</p>
      <strong>Views · State · Product logic</strong>
      <span>One typed Rust architecture</span>
    </div>
    <div class="architecture-flow" aria-hidden="true"><span></span><span></span><span></span><span></span></div>
    <div class="architecture-targets">
      <div><span>macOS</span><strong>Metal</strong></div>
      <div><span>Windows</span><strong>Direct3D 11</strong></div>
      <div><span>Linux</span><strong>Vulkan</strong></div>
      <div><span>Browser</span><strong>WebGL2</strong></div>
    </div>
    <p class="figure-caption">The same retained scene, adapted at the platform boundary.</p>
  </section>

  <section class="home-chapter founder-chapter" aria-labelledby="why-title">
    <div class="chapter-index">01</div>
    <div class="chapter-heading">
      <p class="eyebrow">Why I created Kael</p>
      <h2 id="why-title">Powerful software should not need a stack of runtimes.</h2>
    </div>
    <div class="chapter-copy">
      <p>I wanted a foundation for the kind of applications I care about building: documents, sheets, presentations, whiteboards, creative tools, engines, and workspaces that stay fast as they become more capable.</p>
      <p>I also wanted the web build to be the same product, not a second interface maintained beside the first. Kael keeps rendering, state, input, accessibility, documents, platform services, testing, and release engineering inside one Rust system.</p>
      <p class="signature">Augustus Otu<br><span>Creator of Kael</span></p>
      <a class="text-link" href="why-kael.html">Read the full story <span aria-hidden="true">→</span></a>
    </div>
  </section>

  <section class="home-chapter today-chapter" aria-labelledby="today-title">
    <div class="chapter-index">02</div>
    <div class="chapter-heading">
      <p class="eyebrow">What Kael is today</p>
      <h2 id="today-title">A complete application foundation, from first pixel to signed release.</h2>
    </div>
    <div class="capability-rows">
      <a href="core-concepts.html"><span>Foundation</span><strong>Retained UI, reactive entities, layout, text, input, windows, accessibility</strong><i aria-hidden="true">01</i></a>
      <a href="lists-and-data.html"><span>Scale</span><strong>Virtualization, bounded caches, frame skipping, damage tracking, GPU budgets</strong><i aria-hidden="true">02</i></a>
      <a href="platform-apis.html"><span>Product</span><strong>Files, storage, documents, networking, media, WebViews, notifications, sharing</strong><i aria-hidden="true">03</i></a>
      <a href="releasing.html"><span>Delivery</span><strong>Testing, diagnostics, packaging, signing, updates, platform release gates</strong><i aria-hidden="true">04</i></a>
    </div>
    <a class="text-link" href="framework-today.html">See every framework layer <span aria-hidden="true">→</span></a>
  </section>

  <section class="proof-chapter" aria-labelledby="proof-title">
    <div class="proof-intro">
      <p class="eyebrow">Proof, not promises</p>
      <h2 id="proof-title">Scale is part of the release contract.</h2>
      <p>Kael’s maintained workloads exercise the actual retained rendering path on native and browser targets. The limits are checked in release tooling, not estimated from isolated widgets.</p>
    </div>
    <div class="proof-grid">
      <div><strong>1,000,000</strong><span>logical table rows</span><small>64 or fewer mounted</small></div>
      <div><strong>100,000</strong><span>whiteboard shapes</span><small>spatially culled</small></div>
      <div><strong>34</strong><span>publishable crates</span><small>one release line</small></div>
      <div><strong>4</strong><span>GPU render targets</span><small>one retained scene</small></div>
    </div>
    <a class="text-link" href="suite-scale-apps.html">Inspect the suite-scale workload <span aria-hidden="true">→</span></a>
  </section>

  <section class="home-chapter parity-chapter" aria-labelledby="parity-title">
    <div class="chapter-index">03</div>
    <div class="chapter-heading">
      <p class="eyebrow">Desktop and web</p>
      <h2 id="parity-title">Shared by default. Explicit at the boundary.</h2>
    </div>
    <div class="chapter-copy">
      <p>Application state, layout, components, painting, virtualization, animation, document bytes, workers, and most product services compile from the same source. Native-only abilities remain visible through typed capability reports, so fallbacks are intentional.</p>
      <div class="parity-list">
        <p><span>Shared</span> Views, state, scenes, components, input, files, documents, realtime networking</p>
        <p><span>Adapted</span> Windows, GPU presentation, storage, printing, capture, audio, WebViews</p>
        <p><span>Explicit</span> Subprocesses, arbitrary native paths, system keychains, detached OS windows</p>
      </div>
      <a class="text-link" href="one-codebase.html">Understand the portability contract <span aria-hidden="true">→</span></a>
    </div>
  </section>

  <section class="quickstart-chapter" aria-labelledby="start-title">
    <div>
      <p class="eyebrow">Start with a real app</p>
      <h2 id="start-title">From an empty directory to two targets.</h2>
      <p>Kael’s CLI generates a project whose entry point is already arranged for native and browser builds.</p>
    </div>
    <pre aria-label="Kael quick start commands"><code><span>$</span> cargo install kael-cli<br><span>$</span> kael new my_app<br><span>$</span> cd my_app<br><br><span class="comment"># Native desktop</span><br><span>$</span> cargo run<br><br><span class="comment"># Browser</span><br><span>$</span> kael web serve</code></pre>
    <div class="quickstart-actions">
      <a class="button-primary" href="getting-started.html">Open the guide</a>
      <a class="button-secondary" href="https://docs.rs/kael">Browse the Rust API <span aria-hidden="true">→</span></a>
    </div>
  </section>

  <section class="paths-chapter" aria-labelledby="paths-title">
    <p class="eyebrow">Choose your path</p>
    <h2 id="paths-title">Build the product, not the plumbing.</h2>
    <div class="path-links">
      <a href="component-library.html"><span>Design a product UI</span><small>Components, themes, editors, charts, overlays</small></a>
      <a href="browser.html"><span>Ship to the browser</span><small>WebAssembly, WebGL2, IME, accessibility, workers</small></a>
      <a href="suite-scale-apps.html"><span>Build a productivity suite</span><small>Docs, sheets, slides, whiteboards, large data</small></a>
      <a href="game-input.html"><span>Build an engine or simulation</span><small>Fixed-step clocks, rich input, retained scenes</small></a>
    </div>
  </section>

  <footer class="home-footer">
    <p><strong>Kael</strong> is open source under Apache-2.0. <a href="https://github.com/Augani/kael" target="_blank" rel="noopener noreferrer">View the source on GitHub</a>.</p>
    <p>Kael began as a fork of GPUI by Zed Industries and is now an independent project. It is not affiliated with or endorsed by Zed Industries.</p>
  </footer>
</div>
