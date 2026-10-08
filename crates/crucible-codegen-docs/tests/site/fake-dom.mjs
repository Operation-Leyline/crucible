// A tiny DOM, just enough for explorer.js's `mount` and `start`, so the tests
// can run the real boot path under Node without a browser or npm packages.
// Every place that could interpret markup is recorded: `innerHTML` writes are
// collected on `window.htmlWrites`.

class Node {
  constructor() {
    this.children = [];
    this.parent = null;
    this.listeners = {};
  }
  appendChild(child) {
    child.parent = this;
    this.children.push(child);
    return child;
  }
  append(...nodes) {
    for (const n of nodes) this.appendChild(typeof n === "string" ? new Text(n) : n);
  }
  replaceChildren(...nodes) {
    this.children = [];
    this.append(...nodes);
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  dispatch(type, event = {}) {
    const e = { type, target: this, preventDefault() { this.defaultPrevented = true; }, ...event };
    for (let n = this; n; n = n.parent) for (const fn of n.listeners[type] || []) fn({ ...e, currentTarget: n });
    return e;
  }
}

class Text extends Node {
  constructor(data) {
    super();
    this.data = data;
  }
  get textContent() {
    return this.data;
  }
}

const SIMPLE = /^([a-z0-9]+)?((?:\.[\w-]+)*)(#[\w-]+)?(\[[\w-]+\])?$/i;

export class Element extends Node {
  constructor(tag, doc) {
    super();
    this.tagName = tag.toUpperCase();
    this.doc = doc;
    this.attributes = {};
    this._html = null;
    this.tabIndex = -1;
  }
  setAttribute(name, value) {
    this.attributes[name] = String(value);
  }
  getAttribute(name) {
    return name in this.attributes ? this.attributes[name] : null;
  }
  hasAttribute(name) {
    return name in this.attributes;
  }
  set id(v) { this.setAttribute("id", v); }
  get id() { return this.attributes.id || ""; }
  set className(v) { this.setAttribute("class", v); }
  get className() { return this.attributes.class || ""; }
  set href(v) { this.setAttribute("href", v); }
  set type(v) { this.setAttribute("type", v); }
  set placeholder(v) { this.setAttribute("placeholder", v); }
  set innerHTML(html) {
    this._html = html;
    this.children = [];
    this.doc.htmlWrites.push({ element: this, html });
  }
  get textContent() {
    return this._html != null ? "" : this.children.map((c) => c.textContent).join("");
  }
  set textContent(v) {
    this._html = null;
    this.children = [];
    this.appendChild(new Text(String(v)));
  }
  focus() { this.doc.activeElement = this; }
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this);
  }
  click() { this.dispatch("click"); }
  select() {}
  scrollIntoView() { this.doc.scrolledTo = this; }
  matches(selector) {
    const [, tag, classes, id, attribute] = SIMPLE.exec(selector) || [];
    if (tag && this.tagName !== tag.toUpperCase()) return false;
    const have = this.className.split(/\s+/);
    if (classes && !classes.split(".").filter(Boolean).every((c) => have.includes(c))) return false;
    if (id && this.id !== id.slice(1)) return false;
    if (attribute && !this.hasAttribute(attribute.slice(1, -1))) return false;
    return true;
  }
  closest(selector) {
    for (let n = this; n instanceof Element; n = n.parent) if (n.matches(selector)) return n;
    return null;
  }
  *descendants() {
    for (const child of this.children) {
      if (child instanceof Element) {
        yield child;
        yield* child.descendants();
      }
    }
  }
  querySelectorAll(selector) {
    const parts = selector.split(/\s+/);
    return [...this.descendants()].filter((el) => {
      if (!el.matches(parts[parts.length - 1])) return false;
      let at = parts.length - 2;
      for (let n = el.parent; n instanceof Element && at >= 0; n = n.parent) if (n.matches(parts[at])) at -= 1;
      return at < 0;
    });
  }
  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }
}

export function createWindow({ hash = "", description } = {}) {
  const doc = new Element("html", null);
  doc.doc = doc;
  doc.htmlWrites = [];
  doc.createElement = (tag) => new Element(tag, doc);
  doc.createTextNode = (data) => new Text(data);
  doc.getElementById = (id) => [...doc.descendants()].find((e) => e.id === id) || null;
  doc.body = new Element("body", doc);
  doc.appendChild(doc.body);
  doc.title = "";
  doc.activeElement = null;
  doc.readyState = "complete";
  const listeners = {};
  const win = {
    document: doc,
    location: { hash },
    navigator: { clipboard: { copied: [], writeText(v) { this.copied.push(v); return Promise.resolve(); } } },
    CRUCIBLE_DESCRIPTION: description,
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
    setHash(next) {
      win.location.hash = next;
      for (const fn of listeners.hashchange || []) fn({});
    },
    setTimeout: () => 0,
    clearTimeout: () => {},
    scrollTo: () => {},
  };
  const app = doc.createElement("div");
  app.setAttribute("data-crucible-explorer", "");
  doc.body.appendChild(app);
  return { win, doc, app };
}
