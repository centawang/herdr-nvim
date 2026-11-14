-- Invoked only by the private native APP_BIN fixture; no real Herdr/hc/UI.
vim.opt.rtp:append(assert(vim.env.HERDR_NVIM_PLUGIN_ROOT))
local exec = require('herdr-nvim.exec')
assert(exec.is_app(), 'sidecar HOST did not reach nvim')
local vars = {}
for _, key in ipairs({ 'HERDR_NVIM_SELF', 'HERDR_NVIM_PLUGIN_ROOT', 'HERDR_APP_BIN', 'HERDR_APP_HOME', 'HERDR_NVIM_HOST', 'HC_SESSION', 'HC_SOCKET' }) do
  vars[key] = vim.env[key] or vim.NIL
end
local out = exec.default_exec({ exec.self(), 'app-files', 'text', assert(vim.env.HNV_PROBE_TEXT) })
assert(out.code == 0, out.stderr)
vars.text = vim.json.decode(out.stdout).text
vim.fn.writefile({ vim.json.encode(vars) }, assert(vim.env.HNV_CONTEXT_OUTPUT))
