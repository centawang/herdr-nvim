// App wire paths: slash-separated Windows drive/UNC paths, unchanged Unix
// filenames (a backslash there is not a separator). Roots never become C:.
export function wire(path) {
  if (!path) return path;
  if (path.startsWith("\\\\?\\UNC\\")) return "//" + path.slice(8).replace(/\\/g, "/");
  else if (/^\\\\\?\\[a-z]:/i.test(path)) path = path.slice(4);
  if (!windows(path)) return path;
  path = path.replace(/\\/g, "/");
  // Drive letters are case-insensitive even in case-sensitive directories.
  return /^[a-z]:\//i.test(path) ? path[0].toUpperCase() + path.slice(1) : path;
}
function windows(path) { return /^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\") || /^\/\/[^/]+\/[^/]+/.test(path); }
export function rootOf(path) {
  path = wire(path);
  if (/^[a-z]:\//i.test(path)) return path.slice(0, 3);
  const unc = /^(\/\/[^/]+\/[^/]+)(?:\/|$)/.exec(path);
  return unc && !unc[1].slice(2).split("/").some(p => p === "." || p === "..") ? unc[1] : path.startsWith("/") ? "/" : "";
}
export function normalize(path) {
  path = wire(path);
  const root = rootOf(path), parts = [];
  for (const part of path.slice(root.length).split("/")) {
    if (part === "..") { if (parts.length) parts.pop(); }
    else if (part && part !== ".") parts.push(part);
  }
  return root + (root && !root.endsWith("/") && parts.length ? "/" : "") + parts.join("/");
}
export function join(dir, name) {
  dir = wire(dir);
  if (windows(dir)) name = name.replace(/\\/g, "/");
  // Absolute destinations remain absolute; containment is checked in fs.lua
  // against real paths and requires confirmation, not guessed in the mod.
  // The parent determines path style. A Unix entry like C:\\notes.md or
  // \\\\notes.md is a literal name, not an independently Windows-absolute path.
  if (windows(dir) ? rootOf(name) : name.startsWith("/")) return normalize(name);
  return normalize(`${dir.replace(/\/$/, "")}/${name}`);
}
export function dirname(path) {
  path = normalize(path);
  const root = rootOf(path);
  if (path === root) return root;
  const at = path.lastIndexOf("/");
  return at < root.length ? root : path.slice(0, at);
}
export function basename(path) { path = normalize(path); return path.slice(dirname(path).length).replace(/^\//, "") || path; }
export function inside(path, root) {
  path = normalize(path); root = normalize(root);
  return path === root || path.startsWith(root.endsWith("/") ? root : `${root}/`);
}
export function relative(path, root) {
  path = normalize(path); root = normalize(root);
  return inside(path, root) ? path.slice(root.length).replace(/^\//, "") : null;
}
export function same(a, b) { return a != null && b != null && normalize(a) === normalize(b); }
