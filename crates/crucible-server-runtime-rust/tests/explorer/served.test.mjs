import test from "node:test";
import assert from "node:assert/strict";
import { runInNewContext } from "node:vm";
import { createWindow } from "../../../crucible-codegen-docs/tests/site/fake-dom.mjs";

test("served assets boot the renderer extension and call the normal ping route", { skip: !process.env.EXPLORER_URL }, async () => {
  const base = process.env.EXPLORER_URL.replace(/\/$/, "");
  const { win, doc, app } = createWindow({ hash: "#/op/example.weather%23Ping" });
  const createElement = doc.createElement;
  doc.createElement = (tag) => { const element = createElement(tag); element.ownerDocument = doc; return element; };
  doc.readyState = "loading";
  const calls = [];
  Object.assign(win, {
    location: { href: `${base}/explorer/`, hash: "#/op/example.weather%23Ping" },
    sessionStorage: { getItem: () => null, setItem: () => {} },
    Headers, URL, URLSearchParams, TextEncoder, Uint8Array, structuredClone,
    crypto: globalThis.crypto, btoa, atob,
    fetch: async (url, options) => { calls.push(url); return fetch(url, options); },
  });
  const index = await fetch(`${base}/explorer/`);
  assert.equal(index.status, 200);
  const html = await index.text();
  for (const [, asset] of html.matchAll(/<script src="([^"]+)" defer><\/script>/g)) {
    const response = await fetch(`${base}/explorer/${asset}`);
    assert.equal(response.status, 200, asset);
    runInNewContext(await response.text(), win, { filename: asset });
  }
  assert.equal(win.CrucibleExplorer.operationPanels.length, 1);
  await win.CrucibleExplorer.boot(win);
  assert.match(app.textContent, /Try it/);
  assert.match(app.textContent, /Raw JSON/);
  const form = app.querySelector("form");
  assert.ok(form);
  await form.listeners.submit[0]({ preventDefault() {} });
  assert.deepEqual(calls, [`${base}/ping`]);
  assert.match(app.textContent, /HTTP 200/);
});
