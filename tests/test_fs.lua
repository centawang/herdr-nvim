-- The native tree's file operations, done by nvim (lua/herdr-nvim/fs.lua).
-- Headless nvim without Snacks: herdr-nvim's own rename/delete path.
local fs = require("herdr-nvim.fs")

local function tmpdir()
  local dir = vim.fs.normalize(vim.fn.resolve(vim.fn.tempname()))
  vim.fn.mkdir(dir .. "/src", "p")
  vim.fn.writefile({ "hello" }, dir .. "/src/a.txt")
  return dir
end

local function run(req)
  return vim.json.decode(fs.run(vim.json.encode(req)))
end

T.test("fs: create makes a folder, or a file opened in nvim", function()
  local root = tmpdir()
  T.eq(run({ op = "create", root = root, path = root .. "/src/new/", folder = true }).ok, true)
  T.eq(vim.fn.isdirectory(root .. "/src/new"), 1)
  T.eq(run({ op = "create", root = root, path = root .. "/src/new/b.lua" }).ok, true)
  T.eq(vim.fn.filereadable(root .. "/src/new/b.lua"), 1)
  T.eq(vim.fn.expand("%:t"), "b.lua")
  T.ok(run({ op = "create", root = root, path = root .. "/src/a.txt" }).error:find("already exists"))
end)

T.test("fs: rename moves the open buffer with the file", function()
  local root = tmpdir()
  vim.cmd.edit(root .. "/src/a.txt")
  local r = run({ op = "rename", root = root, path = root .. "/src/a.txt", to = root .. "/src/deep/b.txt" })
  T.eq(r.ok, true)
  T.eq(vim.fn.filereadable(root .. "/src/deep/b.txt"), 1)
  T.eq(vim.fn.filereadable(root .. "/src/a.txt"), 0)
  T.eq(vim.fs.normalize(vim.fn.expand("%:p")), root .. "/src/deep/b.txt")
  T.eq(vim.fn.bufnr(root .. "/src/a.txt"), -1)
end)

T.test("fs: renaming a folder moves the buffers inside it", function()
  local root = tmpdir()
  vim.cmd.edit(root .. "/src/a.txt")
  T.eq(run({ op = "rename", root = root, path = root .. "/src", to = root .. "/lib" }).ok, true)
  T.eq(vim.fs.normalize(vim.fn.expand("%:p")), root .. "/lib/a.txt")
end)

T.test("fs: a rename keeps unsaved edits, undo, and a plain :w", function()
  local root = tmpdir()
  vim.cmd.edit(root .. "/src/a.txt")
  local buf = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "unsaved" })
  T.eq(run({ op = "rename", root = root, path = root .. "/src/a.txt", to = root .. "/src/b.txt" }).ok, true)
  T.eq(vim.api.nvim_get_current_buf(), buf)
  T.eq(vim.fs.normalize(vim.api.nvim_buf_get_name(buf)), root .. "/src/b.txt")
  T.eq(vim.bo[buf].modified, true)
  T.eq(vim.api.nvim_buf_get_lines(buf, 0, -1, false), { "unsaved" })
  T.eq(vim.fn.readfile(root .. "/src/b.txt"), { "hello" })
  T.eq(vim.fn.bufnr(root .. "/src/a.txt"), -1)
  vim.cmd("silent write")
  T.eq(vim.fn.readfile(root .. "/src/b.txt"), { "unsaved" })
  vim.cmd("silent undo")
  T.eq(vim.api.nvim_buf_get_lines(buf, 0, -1, false), { "hello" })
end)

T.test("fs: a folder rename keeps the unsaved edits of a hidden buffer", function()
  local root = tmpdir()
  vim.cmd.edit(root .. "/src/a.txt")
  local buf = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "unsaved" })
  vim.cmd("hide enew")
  T.eq(run({ op = "rename", root = root, path = root .. "/src", to = root .. "/lib" }).ok, true)
  T.eq(vim.fs.normalize(vim.api.nvim_buf_get_name(buf)), root .. "/lib/a.txt")
  T.eq(vim.bo[buf].modified, true)
  T.eq(vim.api.nvim_buf_get_lines(buf, 0, -1, false), { "unsaved" })
end)

T.test("fs: move and copy into a folder, copy to a name", function()
  local root = tmpdir()
  vim.fn.mkdir(root .. "/dest", "p")
  T.eq(run({ op = "copy", root = root, path = root .. "/src/a.txt", to = root .. "/src/c.txt" }).ok, true)
  T.eq(vim.fn.readfile(root .. "/src/c.txt"), { "hello" })
  T.eq(run({ op = "copy_into", root = root, paths = { root .. "/src/a.txt" }, dir = root .. "/dest" }).ok, true)
  T.eq(vim.fn.filereadable(root .. "/dest/a.txt"), 1)
  T.eq(run({ op = "move", root = root, paths = { root .. "/src/c.txt" }, dir = root .. "/dest" }).ok, true)
  T.eq(vim.fn.filereadable(root .. "/dest/c.txt"), 1)
  T.eq(vim.fn.filereadable(root .. "/src/c.txt"), 0)
end)

T.test("fs: delete removes the file and wipes its buffer", function()
  local root = tmpdir()
  local file = root .. "/src/a.txt"
  vim.cmd.edit(file)
  vim.cmd.enew()
  T.ok(vim.fn.bufnr(file) > 0)
  T.eq(run({ op = "delete", root = root, paths = { root .. "/src" } }).ok, true)
  T.eq(vim.fn.isdirectory(root .. "/src"), 0)
  T.eq(vim.fn.bufnr(file), -1)
end)

T.test("fs: yank then paste copies through nvim's register", function()
  local root = tmpdir()
  vim.fn.mkdir(root .. "/dest", "p")
  T.eq(run({ op = "yank", root = root, paths = { root .. "/src/a.txt" } }).count, 1)
  T.eq(run({ op = "paste", root = root, dir = root .. "/dest" }).ok, true)
  T.eq(vim.fn.filereadable(root .. "/dest/a.txt"), 1)
end)

T.test("fs: nothing outside the tab's folder without the person's yes", function()
  local root = tmpdir()
  local other = tmpdir()
  local r = run({ op = "copy", root = root, path = root .. "/src/a.txt", to = other .. "/x.txt" })
  T.ok(r.error and r.error:find("outside"))
  T.eq(vim.fn.filereadable(other .. "/x.txt"), 0)
  T.eq(run({ op = "copy", root = root, outside = true, path = root .. "/src/a.txt", to = other .. "/x.txt" }).ok, true)
end)

T.test("fs: a symlink in the root is itself: renaming or deleting it touches only the link", function()
  local root = tmpdir()
  local other = tmpdir()
  vim.fn.writefile({ "keep" }, other .. "/kept.txt")
  local linked = vim.uv.fs_symlink(other, root .. "/link", vim.fn.has("win32") == 1 and { junction = true } or nil)
  T.ok(linked, "could not create test link/junction")
  local r = run({ op = "rename", root = root, path = root .. "/link", to = root .. "/link2" })
  T.eq(r.ok, true, vim.inspect(r))
  T.eq(vim.uv.fs_lstat(root .. "/link2").type, "link")
  r = run({ op = "delete", root = root, paths = { root .. "/link2" } })
  T.eq(r.ok, true, vim.inspect(r))
  T.eq(vim.uv.fs_lstat(root .. "/link2"), nil)
  T.eq(vim.fn.readfile(other .. "/kept.txt"), { "keep" }, "the folder it pointed to is untouched")
end)

T.test("fs: going through a symlink to elsewhere asks first (outside)", function()
  local root = tmpdir()
  local other = tmpdir()
  local linked = vim.uv.fs_symlink(other, root .. "/link", vim.fn.has("win32") == 1 and { junction = true } or nil)
  T.ok(linked, "could not create test link/junction")
  local r = run({ op = "create", root = root, path = root .. "/link/x.txt" })
  T.eq(r.outside, root .. "/link/x.txt", vim.inspect(r))
end)

T.test("fs: a symlink in the root to elsewhere is outside", function()
  local root = tmpdir()
  local other = tmpdir()
  local linked = vim.uv.fs_symlink(other, root .. "/link", vim.fn.has("win32") == 1 and { junction = true } or nil)
  T.ok(linked, "could not create test link/junction")
  local r = run({ op = "create", root = root, path = root .. "/link/new.txt" })
  T.ok(r.error and r.error:find("outside"), vim.inspect(r))
  T.eq(vim.fn.filereadable(other .. "/new.txt"), 0)
  r = run({ op = "rename", root = root, path = root .. "/src/a.txt", to = root .. "/link/deep/a.txt" })
  T.ok(r.error and r.error:find("outside"), vim.inspect(r))
  T.eq(vim.fn.filereadable(root .. "/src/a.txt"), 1)
  T.eq(run({ op = "create", root = root, path = root .. "/src/new/x.txt" }).ok, true)
end)

T.test("fs: recursive copy is native and refuses self-descendants and links", function()
  local root = tmpdir()
  T.eq(run({ op = "copy", root = root, path = root .. "/src", to = root .. "/copy" }).ok, true)
  T.eq(vim.fn.readfile(root .. "/copy/a.txt"), { "hello" })
  T.ok(run({ op = "copy", root = root, path = root .. "/src", to = root .. "/src/nested" }).error)
  local uv = vim.uv or vim.loop
  assert(uv.fs_symlink(root .. "/src", root .. "/copy/link", vim.fn.has("win32") == 1 and { junction = true } or nil))
  T.ok(run({ op = "copy", root = root, path = root .. "/copy", to = root .. "/rejected" }).error)
  T.eq(vim.fn.isdirectory(root .. "/rejected"), 0)
end)

T.test("fs: private copies stage owner-only directories, including partial failure", function()
  local uv = vim.uv or vim.loop
  local root = tmpdir()
  vim.fn.mkdir(root .. "/src/private", "p")
  vim.fn.writefile({ "secret" }, root .. "/src/private/secret.txt")
  local unix = vim.fn.has("win32") == 0
  if unix then
    assert(uv.fs_chmod(root .. "/src", 448))
    assert(uv.fs_chmod(root .. "/src/private", 448))
    assert(uv.fs_chmod(root .. "/src/private/secret.txt", 420))
  end
  local mkdir, copyfile = uv.fs_mkdir, uv.fs_copyfile
  local modes = {}
  uv.fs_mkdir = function(path, mode, ...)
    modes[#modes + 1] = mode
    return mkdir(path, mode, ...)
  end
  local r = run({ op = "copy", root = root, path = root .. "/src", to = root .. "/copied" })
  uv.fs_mkdir = mkdir
  T.eq(r.ok, true, vim.inspect(r))
  for _, mode in ipairs(modes) do T.eq(mode, 448, "construction must request 0700, not 0755") end
  T.eq(vim.fn.readfile(root .. "/copied/private/secret.txt"), { "secret" })
  if unix then
    T.eq(uv.fs_stat(root .. "/copied").mode % 512, 448)
    T.eq(uv.fs_stat(root .. "/copied/private").mode % 512, 448)
    T.eq(uv.fs_stat(root .. "/copied/private/secret.txt").mode % 512, 420)
  end
  uv.fs_copyfile = function(src, dst, ...)
    if src:find("secret.txt", 1, true) then return nil, "injected copy failure" end
    return copyfile(src, dst, ...)
  end
  r = run({ op = "copy", root = root, path = root .. "/src", to = root .. "/partial" })
  uv.fs_copyfile = copyfile
  T.ok(r.error and r.error:find("injected copy failure"))
  if unix then
    T.eq(uv.fs_stat(root .. "/partial").mode % 512, 448)
    T.eq(uv.fs_stat(root .. "/partial/private").mode % 512, 448)
    for _, op in ipairs({ "copy_into", "paste" }) do
      local dest = root .. "/" .. op
      vim.fn.mkdir(dest, "p")
      if op == "paste" then T.eq(run({ op = "yank", root = root, paths = { root .. "/src" } }).ok, true) end
      T.eq(run({ op = op, root = root, paths = { root .. "/src" }, dir = dest }).ok, true)
      T.eq(uv.fs_stat(dest .. "/src").mode % 512, 448)
      T.eq(uv.fs_stat(dest .. "/src/private").mode % 512, 448)
    end
  else
    print("NOTE: Unix permission assertions not executed on Windows")
  end
end)
