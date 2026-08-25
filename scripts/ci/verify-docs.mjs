#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const workspace = process.cwd();
const sourceRoot = path.join(workspace, "docs", "src");
const outputRoot = path.resolve(process.argv[2] ?? path.join("target", "book"));
const failures = [];

function fail(message) {
  failures.push(message);
}

function filesBelow(root, predicate) {
  const files = [];
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(absolute);
      else if (predicate(absolute)) files.push(absolute);
    }
  }
  visit(root);
  return files;
}

if (!fs.existsSync(outputRoot)) {
  fail(`missing mdBook output: ${outputRoot}`);
}

const summaryPath = path.join(sourceRoot, "SUMMARY.md");
const summary = fs.readFileSync(summaryPath, "utf8");
const listedPages = new Set(
  [...summary.matchAll(/\]\(([^)#]+\.md)\)/g)].map((match) => path.normalize(match[1])),
);
const sourcePages = filesBelow(sourceRoot, (file) => file.endsWith(".md"))
  .map((file) => path.relative(sourceRoot, file))
  .filter((file) => file !== "SUMMARY.md");

for (const page of sourcePages) {
  if (!listedPages.has(page)) fail(`page is missing from SUMMARY.md: ${page}`);
}
for (const page of listedPages) {
  if (!sourcePages.includes(page)) fail(`SUMMARY.md points to a missing page: ${page}`);
}

const requiredPages = [
  "framework-today.md",
  "object-guide.md",
  "one-codebase.md",
  "browser.md",
  "web-deployment.md",
  "remaining-work.md",
  "llms.md",
];
for (const page of requiredPages) {
  if (!listedPages.has(page)) fail(`release guide is not discoverable: ${page}`);
}

const quickstartInclude = "{{#include ../../crates/kael_ui/examples/docs_quickstart.rs}}";
for (const page of ["getting-started.md", "one-codebase.md"]) {
  const source = fs.readFileSync(path.join(sourceRoot, page), "utf8");
  if (!source.includes(quickstartInclude)) {
    fail(`${page} does not use the compiled quick-start source`);
  }
}

const llmsSource = fs.readFileSync(path.join(workspace, "llms.txt"), "utf8");
for (const reference of [
  "object-guide.html",
  "one-codebase.html",
  "browser.html",
  "web-deployment.html",
  "remaining-work.html",
  "crates/kael/src/platform_caps.rs",
  "crates/kael-cli/src/web.rs",
]) {
  if (!llmsSource.includes(reference)) fail(`llms.txt is missing ${reference}`);
}

const homeSource = fs.readFileSync(path.join(sourceRoot, "index.md"), "utf8");
if (!homeSource.includes('href="https://github.com/Augani/kael"')) {
  fail("home page is missing the labeled GitHub repository link");
}

if (fs.existsSync(outputRoot)) {
  const htmlFiles = filesBelow(outputRoot, (file) => file.endsWith(".html"));
  for (const htmlFile of htmlFiles) {
    const html = fs.readFileSync(htmlFile, "utf8");
    const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map((match) => match[1]);
    const duplicateIds = ids.filter((id, index) => ids.indexOf(id) !== index);
    for (const id of new Set(duplicateIds)) {
      fail(`${path.relative(outputRoot, htmlFile)} has duplicate id #${id}`);
    }

    for (const match of html.matchAll(/href="([^"]+)"/g)) {
      const href = match[1];
      if (/^(?:https?:|mailto:|javascript:|#)/.test(href)) continue;
      const clean = decodeURIComponent(href.split("#")[0].split("?")[0]);
      if (!clean || clean === "/") continue;
      let target = path.resolve(path.dirname(htmlFile), clean);
      if (target.endsWith(path.sep)) target = path.join(target, "index.html");
      if (!fs.existsSync(target)) {
        fail(`${path.relative(outputRoot, htmlFile)} has broken link ${href}`);
      }
    }
  }
}

const fontRoot = path.join(sourceRoot, "fonts");
const fontFiles = filesBelow(fontRoot, (file) => /Inter-.*\.woff$/.test(file));
const fontBytes = fontFiles.reduce((total, file) => total + fs.statSync(file).size, 0);
if (fontFiles.length !== 3) fail(`expected 3 Inter WOFF faces, found ${fontFiles.length}`);
if (fontBytes > 600 * 1024) fail(`Inter webfonts exceed 600 KiB: ${fontBytes} bytes`);
if (filesBelow(fontRoot, (file) => /\.(?:ttf|otf|woff2)$/.test(file)).length > 0) {
  fail("docs font directory contains an unexpected uncompressed or unapproved font");
}

if (failures.length > 0) {
  console.error("Documentation verification failed:");
  for (const message of failures) console.error(`  - ${message}`);
  process.exit(1);
}

console.log(
  `Documentation verification passed: ${sourcePages.length} pages, ` +
    `${fontBytes} font bytes, no broken local links or duplicate ids.`,
);
