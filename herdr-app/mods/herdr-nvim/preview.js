// The preview: a file shown as what it is, in the tab's nvim's place
// (the controller, sync.js `startSwap`, splits nvim's pane with this mod, props
// {preview: <file>, tab}, then closes nvim's pane; nvim keeps running).
// Markdown is rendered, images and SVGs drawn. It reloads when the file
// changes (saved in nvim, written by an agent). The pane header's Source
// button (mods/herdr-nvim-header) puts nvim back.
//
// Images are drawn with image(), which takes a path inside the app home: a
// copy goes to `mods/herdr-nvim/.cache` (hidden, so writing there reloads no
// mod), named by its content, the newest CACHE_KEEP kept.

import { div, image } from "gpui-kit";
import { v_flex, TextView } from "gpui-base";
import { appHome, exec, panes, setShared, shared, theme, watchFolder } from "herdr";
import { bin } from "./lib.js";
import { dirname, wire, same } from "./paths.js";
import * as fs from "fs/promises";

const KINDS = {
  md: "markdown", markdown: "markdown", mdx: "markdown",
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", bmp: "image", ico: "image", svg: "image",
};
const CACHE = "mods/herdr-nvim/.cache";
const CACHE_KEEP = 40;
// shared(): which file each tab's preview shows, for its pane header.
const SHOWN = "herdr-nvim.previews";

// "markdown", "image", or null: what a file previews as.
export function previewKind(path) {
  const ext = /\.([^./]+)$/.exec(path ?? "")?.[1]?.toLowerCase();
  return (ext && KINDS[ext]) || null;
}



async function keepCached(src) {
  const name = src.slice(CACHE.length + 1);
  const valid = (n) => /^[0-9a-f]+\.[a-z0-9]+$/.test(n);
  if (!src.startsWith(`${CACHE}/`) || !valid(name)) throw new Error("Invalid image cache path");
  const keep = [name, ...((shared("herdr-nvim.preview-cache") ?? []).filter((n) => n !== name && valid(n)))];
  for (const old of keep.slice(CACHE_KEEP)) await fs.unlink(`${appHome()}/${CACHE}/${old}`).catch(() => {});
  setShared("herdr-nvim.preview-cache", keep.slice(0, CACHE_KEEP));
  return src;
}

// A mod reads files only inside the home folder: one elsewhere (/tmp, a
// screenshot under /private/var/folders) is read through a process.
async function readNative(file, op) {
  const r = await exec(bin(), ["app-files", op, file]);
  if (r.code !== 0) throw new Error(r.stderr.trim() || "Cannot read preview");
  return JSON.parse(r.stdout);
}
// Rust enforces byte limits before returning text or caching image bytes.
// Large images never travel through the app's exec stdout transport.
export async function load(file) {
  const kind = previewKind(file);
  if (!kind) return { error: "Can't preview this kind of file." };
  try {
    if (kind === "markdown") return { kind, text: (await readNative(file, "text")).text };
    return { kind, src: await keepCached((await readNative(file, "image")).src) };
  } catch (e) {
    return { error: String(e.message || e) };
  }
}

export class Preview {
  constructor(props, cx) {
    this.file = wire(props.preview);
    this.tab = props.tab;
    this.shown = null;
    // Only tabs that still show a preview keep their entry.
    const showing = new Set(panes().filter((p) => p.terminal === null && p.mod === "herdr-nvim").map((p) => p.tab));
    const kept = Object.fromEntries(Object.entries(shared(SHOWN) ?? {}).filter(([tab]) => showing.has(tab)));
    setShared(SHOWN, { ...kept, [this.tab]: this.file });
    cx.spawn(async (cx) => {
      // A changed path of the folder's root means "anything may have changed".
      const ours = (changed) => !changed.length || changed.some((p) => same(p, this.file) || same(p, dirname(this.file)));
      while (!this.closed) {
        this.shown = await load(this.file);
        cx.notify();
        while (!this.closed && !ours(await watchFolder(dirname(this.file))));
      }
    });
  }

  // The pane is gone (main.js `close`): the watch loop ends.
  close() {
    this.closed = true;
  }

  render() {
    const t = theme();
    const s = this.shown;
    const body = !s
      ? div()
      : s.error
        ? div().m_auto().text_color(t.dim).child(s.error)
        : s.kind === "image"
          ? div().flex_1().min_h(0).w_full().p(14).child(image(s.src).size_full())
          : TextView.markdown("herdr-nvim-preview", s.text).selectable(true).scrollable(true).flex_1().min_h(0).px(18).py(12);
    return v_flex()
      .id("herdr-nvim-preview")
      .size_full()
      .bg(t.background)
      .text_size(13)
      .text_color(t.foreground)
      .child(body);
  }
}
