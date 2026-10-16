// Extension of the docs renderer. Wire rules are read only from describe/1.
(function (root) {
  "use strict";
  const JSON_PROTOCOL = "crucibleJson";
  const own = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
  const percent = (value) => encodeURIComponent(String(value)).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
  const scalar = (value) => typeof value === "string" ? value : JSON.stringify(value);
  const items = (value) => Array.isArray(value) ? value : [value];

  function serviceBase(page, prefixSegments = 1) {
    const url = new URL(page);
    const parts = url.pathname.split("/").filter(Boolean);
    if (parts.at(-1) === "index.html") parts.pop();
    parts.splice(-prefixSegments);
    url.pathname = `/${parts.join("/")}${parts.length ? "/" : ""}`;
    url.search = url.hash = "";
    return url;
  }

  function callable(operation) {
    return operation.callable !== false && operation.protocol === JSON_PROTOCOL;
  }

  function timestamp(value, format) {
    if (value == null || !format) return value;
    const date = typeof value === "number" ? new Date(value * 1000) : new Date(value);
    if (!Number.isFinite(date.valueOf())) throw new Error(`Invalid timestamp: ${value}`);
    if (format === "epoch-seconds") return date.valueOf() / 1000;
    if (format === "http-date") return date.toUTCString();
    return date.toISOString().replace(/\.000Z$/, "Z");
  }

  function wire(value, member, description, format) {
    if (value == null) return value;
    const shape = description.shapes[member.target];
    if (!shape) throw new Error(`Missing shape ${member.target}`);
    switch (shape.kind) {
      case "timestamp": return timestamp(value, format || member.timestampFormat);
      case "long":
        if (!Number.isSafeInteger(Number(value))) throw new Error("Integers beyond JavaScript's safe range require a native client");
        return Number(value);
      case "bigInteger":
        if (typeof value === "number" && !Number.isSafeInteger(value)) throw new Error("Use a string for integers beyond JavaScript's safe range; this protocol may require a native client");
        return value;
      case "bigDecimal": return value;
      case "list": return value.map((v) => wire(v, shape.members[0], description, format));
      case "map": return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, wire(v, shape.members.find((m) => m.name === "value"), description, format)]));
      case "structure": case "union": {
        const result = {};
        for (const m of shape.members) {
          if (own(value, m.wireName)) {
            const v = wire(value[m.wireName], m, description);
            Object.defineProperty(result, m.wireName, { value: v, enumerable: true, configurable: true, writable: true });
          }
        }
        return result;
      }
      default: return value;
    }
  }

  function base64(text) {
    const bytes = new TextEncoder().encode(text);
    return root.btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
  }

  function headerValue(value, encoding, format) {
    const encode = (v) => encoding ? base64(scalar(v)) : scalar(v);
    if (!Array.isArray(value)) return encode(value);
    return value.filter((v) => v != null).map((v) => {
      const text = encode(v);
      return format === "http-date" || !/[",]/.test(text) ? text : `"${text.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    }).join(", ");
  }

  function buildRequest(description, operation, input, base, credentials = {}, options = {}) {
    if (!callable(operation)) throw new Error("Use a native client or curl for this operation");
    if (options.readOnly && !operation.readonly) throw new Error("Explorer is read-only for this operation");
    if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("Input must be a JSON object");
    const bindings = operation.request;
    const members = (description.shapes[operation.input] || {}).members || [];
    const values = new Map();
    for (const member of members) {
      if (own(input, member.wireName)) values.set(member.name, wire(input[member.wireName], member, description, (bindings.timestampFormats || {})[member.name]));
    }
    if (operation.idempotencyToken && !values.get(operation.idempotencyToken)) {
      values.set(operation.idempotencyToken, root.crypto.randomUUID());
    }
    let uri = operation.uri;
    for (const name of bindings.labels || []) {
      const value = values.get(name);
      if (value == null || value === "") throw new Error(`Required path label: ${name}`);
      const greedy = uri.includes(`{${name}+}`);
      const segments = greedy ? scalar(value).split("/") : [scalar(value)];
      if (segments.some((v) => v === "." || v === "..")) throw new Error("Dot path labels cannot be sent safely by a browser");
      uri = uri.replace(`{${name}${greedy ? "+" : ""}}`, segments.map(percent).join("/"));
    }
    const query = [];
    const addQuery = (name, value) => items(value).filter((v) => v != null).forEach((v) => query.push(`${percent(name)}=${percent(scalar(v))}`));
    for (const binding of bindings.query || []) {
      if (values.get(binding.member) != null) addQuery(binding.name, values.get(binding.member));
    }
    const headers = new Headers(operation.protocolHeaders || {});
    for (const binding of bindings.headers || []) {
      if (values.get(binding.member) != null) headers.set(binding.name, headerValue(values.get(binding.member), (bindings.headerEncodings || {})[binding.member], (bindings.timestampFormats || {})[binding.member]));
    }
    // A sender-signed operation takes no credential of this service.
    if (credentials.bearer && !operation.sender) headers.set("Authorization", `Bearer ${credentials.bearer}`);
    if (query.length) uri += `${uri.includes("?") ? "&" : "?"}${query.join("&")}`;
    const baseUrl = new URL(base);
    const url = new URL(`${baseUrl.origin}${baseUrl.pathname}${uri.replace(/^\/+/, "")}`);
    if (url.origin !== baseUrl.origin || !url.pathname.startsWith(baseUrl.pathname)) throw new Error("Request escaped the service base URL");
    let body;
    if (bindings.payload) {
      const value = values.get(bindings.payload.member);
      if (value != null) {
        if (bindings.payload.kind === "blob") {
          body = Uint8Array.from(root.atob(value), (c) => c.charCodeAt(0));
        } else if (bindings.payload.kind === "string") body = String(value);
        else body = JSON.stringify(value);
      }
    } else if (bindings.body.length) {
      const result = {};
      for (const member of members) {
        if (bindings.body.includes(member.name) && values.has(member.name)) {
          const value = values.get(member.name);
          Object.defineProperty(result, member.wireName, { value, enumerable: true });
        }
      }
      body = JSON.stringify(result);
    }
    if (body !== undefined && !headers.has("Content-Type")) headers.set("Content-Type", bindings.payload ? bindings.payload.mediaType : "application/json");
    if (["GET", "HEAD"].includes(operation.method) && body !== undefined) throw new Error("Browsers cannot send a GET/HEAD body; use curl");
    return { url: url.href, method: operation.method, headers, body };
  }

  const quote = (s) => `'${String(s).replace(/'/g, "'\\''")}'`;
  function curl(request) {
    const parts = ["curl", "-i", "-X", request.method, quote(request.url)];
    for (const [name, value] of request.headers) parts.push("-H", quote(`${name}: ${value}`));
    if (request.body !== undefined) parts.push("--data-binary", request.body instanceof Uint8Array ? "@payload.bin" : quote(request.body));
    return parts.join(" ");
  }

  function errorName(description, operation, response, body) {
    if (response.status < 400) return null;
    const errors = (operation.errors || []).map((id) => description.shapes[id]).filter(Boolean);
    let code = response.headers.get("X-Crucible-Error") || body && (body.__type || body.code);
    if (typeof code === "string") {
      code = code.split(":")[0].split("#").at(-1);
      const match = errors.find((shape) => shape.name === code || shape.error && shape.error.wireCode.split("#").at(-1) === code);
      return match ? match.name : code;
    }
    const matches = errors.filter((shape) => shape.error && shape.error.status === response.status);
    return matches.length === 1 ? matches[0].name : null;
  }

  function resolveSchema(schema, rootSchema) {
    if (schema.$ref) return resolveSchema(rootSchema.$defs[schema.$ref.split("/").at(-1)], rootSchema);
    if (schema.anyOf) return resolveSchema(schema.anyOf.find((v) => v.type !== "null") || {}, rootSchema);
    return schema;
  }

  function nextInput(operation, body, input, description) {
    if (!operation.pagination || !body) return null;
    function wirePath(shapeId, path) {
      let shape = description.shapes[shapeId];
      return path.split(".").map((name) => {
        const member = shape && (shape.members || []).find((m) => m.name === name);
        shape = member && description.shapes[member.target];
        return member ? member.wireName : name;
      });
    }
    const token = wirePath(operation.output, operation.pagination.outputToken).reduce((v, name) => v && v[name], body);
    if (token == null || token === "") return null;
    const result = structuredClone(input);
    const path = wirePath(operation.input, operation.pagination.inputToken);
    let at = result;
    for (const name of path.slice(0, -1)) {
      if (!own(at, name)) Object.defineProperty(at, name, { value: {}, enumerable: true, writable: true });
      at = at[name];
    }
    Object.defineProperty(at, path.at(-1), { value: token, enumerable: true, writable: true, configurable: true });
    return result;
  }

  function panel(operation, container, context) {
    const doc = container.ownerDocument;
    const config = root.CRUCIBLE_EXPLORER_CONFIG || {};
    const description = context.description;
    const base = serviceBase(root.location.href, config.prefixSegments);
    const element = (tag, text, className) => {
      const node = doc.createElement(tag);
      if (text != null) node.textContent = text;
      if (className) node.className = className;
      return node;
    };
    container.append(element("h2", "Try it"), element("p", "Calls use the service's normal routes, authentication and validation. Credentials stay in this browser tab.", "cx-muted"));
    if (!callable(operation)) {
      const headers = new Headers(operation.protocolHeaders || {});
      const request = { url: `${base.href}${operation.uri.replace(/^\/+/, "")}`, method: operation.method, headers, body: new Uint8Array() };
      container.append(element("p", "This stream needs curl or a native client. Supply the modeled labels and a correctly encoded payload.bin."), element("pre", curl(request), "cx-code"));
      return;
    }
    const form = element("form", null, "cx-try-form");
    const status = element("p", "", "cx-try-status");
    status.setAttribute("role", "status");
    const storageKey = `crucible-explorer:${base.href}:${description.service.id}`;
    let credentials = {};
    try { credentials = JSON.parse(root.sessionStorage.getItem(storageKey) || "{}"); } catch (_) { /* Storage may be disabled. */ }
    const saveCredentials = () => { try { root.sessionStorage.setItem(storageKey, JSON.stringify(credentials)); } catch (_) { status.textContent = "Session storage is unavailable; credentials last until navigation."; } };
    const credential = (label, value, update) => {
      const field = element("label", label, "cx-try-field");
      const input = element("input");
      input.type = "password";
      input.autocomplete = "off";
      input.value = value || "";
      input.addEventListener("input", () => { update(input.value); saveCredentials(); });
      field.append(input);
      form.append(field);
    };
    // A third party calls a sender-signed operation (decision 81) and signs
    // it: no credential of this service applies, the signature does.
    if (operation.sender) {
      const note = element("p", `Called by ${operation.sender.name}: no bearer token applies. Sign the request as ${operation.sender.name} does and send the signature in the ${operation.sender.header || "signature"} header, below.`, "cx-try-sender");
      form.append(note);
    } else {
      credential("Bearer token", credentials.bearer, (value) => { credentials.bearer = value; });
    }
    // The renderer resolves a shared or omitted schema (docs/docs.md).
    const renderer = root.CrucibleExplorer;
    const schema = (renderer && renderer.operationSchema ? renderer.operationSchema(description, operation, "inputSchema") : operation.inputSchema) || {};
    const raw = element("textarea", null, "cx-try-json");
    raw.rows = 10;
    raw.spellcheck = false;
    raw.setAttribute("aria-label", "Input JSON (wire member names; blobs are base64)");
    const controls = [];
    const fields = element("fieldset");
    fields.append(element("legend", "Input form"));
    for (const [name, property] of Object.entries(schema.properties || {})) {
      const s = resolveSchema(property, schema);
      const label = element("label", `${name}${(schema.required || []).includes(name) ? " *" : ""}`, "cx-try-field");
      let input;
      if (s.enum) {
        input = element("select");
        input.append(element("option", ""));
        for (const value of s.enum) { const option = element("option", String(value)); option.value = String(value); input.append(option); }
      } else if (s.type === "boolean") {
        input = element("select");
        for (const value of ["", "true", "false"]) { const option = element("option", value); option.value = value; input.append(option); }
      } else if (s.type === "object" || s.type === "array" || s.oneOf || !s.type) {
        input = element("textarea"); input.rows = 3;
      } else {
        input = element("input"); input.type = "text";
        if (["number", "integer"].includes(s.type)) input.inputMode = "decimal";
      }
      input.autocomplete = "off";
      if (s.description) input.title = s.description;
      const parse = (value) => ["integer", "number", "boolean", "object", "array"].includes(s.type) || s.oneOf || !s.type ? JSON.parse(value) : value;
      controls.push({ name, input, parse, complex: s.type === "object" || s.type === "array" || s.oneOf || !s.type });
      input.addEventListener("input", () => {
        try {
          const value = JSON.parse(raw.value);
          if (input.value === "") delete value[name];
          else Object.defineProperty(value, name, { value: parse(input.value), enumerable: true, writable: true, configurable: true });
          raw.value = JSON.stringify(value, null, 2);
          status.textContent = "";
        } catch (error) { status.textContent = error.message; }
      });
      label.append(input); fields.append(label);
    }
    const syncForm = () => {
      try {
        const value = JSON.parse(raw.value);
        for (const control of controls) control.input.value = !own(value, control.name) ? "" : control.complex ? JSON.stringify(value[control.name], null, 2) : String(value[control.name]);
        status.textContent = "";
      } catch (error) { status.textContent = error.message; }
    };
    const example = operation.examples.find((e) => e.inputValues || e.input && e.input.body);
    raw.value = JSON.stringify(example && (example.inputValues || example.input.body) || {}, null, 2);
    syncForm();
    raw.addEventListener("input", syncForm);
    if (operation.examples.some((e) => e.inputValues)) {
      const select = element("select");
      select.setAttribute("aria-label", "Prefill from an example");
      operation.examples.forEach((e, i) => { if (e.inputValues) { const option = element("option", e.title); option.value = String(i); select.append(option); } });
      select.addEventListener("change", () => { raw.value = JSON.stringify(operation.examples[select.value].inputValues, null, 2); syncForm(); });
      form.append(select);
    }
    form.append(fields, element("h3", "Raw JSON"), raw);
    const actions = element("div", null, "cx-try-actions");
    const send = element("button", "Send request"); send.type = "submit";
    const copy = element("button", "Copy as curl"); copy.type = "button";
    const next = element("button", "Next page"); next.type = "button"; next.disabled = true;
    const clear = element("button", "Clear credentials"); clear.type = "button";
    clear.addEventListener("click", () => { credentials = {}; saveCredentials(); form.querySelectorAll('input[type="password"]').forEach((input) => { input.value = ""; }); });
    const responseView = element("pre", "", "cx-code cx-try-response");
    responseView.setAttribute("aria-label", "Response status, headers and body");
    let pageInput = null;
    const request = () => buildRequest(description, operation, JSON.parse(raw.value), base, credentials, config);
    copy.addEventListener("click", async () => {
      try { await root.navigator.clipboard.writeText(curl(request())); status.textContent = "Copied curl command (includes entered credentials)."; }
      catch (error) { status.textContent = error.message; }
    });
    if (config.readOnly && !operation.readonly) { send.disabled = true; status.textContent = "Explorer is read-only; this operation is not @readonly."; }
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      send.disabled = true; next.disabled = true; status.textContent = "Sending…";
      try {
        const input = JSON.parse(raw.value);
        const r = buildRequest(description, operation, input, base, credentials, config);
        const response = await root.fetch(r.url, { method: r.method, headers: r.headers, body: r.body, credentials: "same-origin", redirect: "error" });
        const text = await response.text();
        let body;
        try { body = JSON.parse(text); } catch (_) { body = null; }
        const name = errorName(description, operation, response, body);
        responseView.textContent = `HTTP ${response.status} ${response.statusText}${name ? `\nModeled error: ${name}` : ""}\n${Array.from(response.headers, ([k, v]) => `${k}: ${v}`).join("\n")}\n\n${body == null ? text : JSON.stringify(body, null, 2)}`;
        pageInput = response.ok ? nextInput(operation, body, input, description) : null;
        next.disabled = !pageInput;
        status.textContent = `HTTP ${response.status}`;
      } catch (error) { status.textContent = error.message; }
      finally { send.disabled = Boolean(config.readOnly && !operation.readonly); }
    });
    next.addEventListener("click", () => { if (pageInput) { raw.value = JSON.stringify(pageInput, null, 2); syncForm(); form.requestSubmit(); } });
    actions.append(send, copy, next, clear);
    form.append(actions, status, responseView);
    container.append(form);
  }

  const api = { buildRequest, serviceBase, callable, curl, errorName, nextInput, panel };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else {
    root.CrucibleTryIt = api;
    if (root.CRUCIBLE_EXPLORER_CONFIG.title) root.CRUCIBLE_DESCRIPTION.service.title = root.CRUCIBLE_EXPLORER_CONFIG.title;
    root.CrucibleExplorer.registerOperationPanel(panel);
  }
})(globalThis);
