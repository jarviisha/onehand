-- Deterministic dense-grid workload: 500 cursor, scroll and redraw updates
-- at a nominal 16 ms cadence, then idle. PERF_PHASE receives the phase and
-- measured columns/rows so an external sampler can delimit each interval.
vim.o.swapfile = false
vim.o.shadafile = 'NONE'
vim.o.termguicolors = true
vim.o.guicursor = 'a:block-blinkon0'
vim.o.number = true
vim.o.cursorline = true
vim.o.laststatus = 2
vim.o.scrolloff = 0
vim.cmd('highlight Normal guifg=#d8dee9 guibg=#202530')
local lines = {}
for i = 1, 20000 do lines[i] = string.format('local value_%05d = compute_result(%d, "terminal benchmark") -- matching source text', i, i) end
if os.getenv('PERF_GUIDES') == '1' then
  for i, line in ipairs(lines) do lines[i] = '│   │   ' .. line end
end
vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
vim.bo.filetype = 'lua'
vim.cmd('syntax on')
local tick = 0
local interval = tonumber(vim.env.PERF_INTERVAL_MS) or 16
local ticks = tonumber(vim.env.PERF_TICKS) or 500
assert(interval >= 1 and ticks >= 1)
local timer = vim.uv.new_timer()
local last_phase
local function phase(name)
  if name == last_phase then return end
  last_phase = name
  vim.fn.writefile({name, tostring(vim.o.columns), tostring(vim.o.lines)}, os.getenv('PERF_PHASE'))
end
phase('warmup')
timer:start(3000, interval, vim.schedule_wrap(function()
  tick = tick + 1
  if tick <= ticks then
    phase('cursor')
    vim.api.nvim_win_set_cursor(0, {10 + tick % 2, 12})
  elseif tick <= 2 * ticks then
    phase('scroll')
    vim.cmd('normal! j')
  elseif tick <= 3 * ticks then
    phase('redraw')
    vim.cmd('normal! j')
    vim.cmd('redraw!')
  elseif tick <= 3 * ticks + math.ceil(4000 / interval) then
    phase('idle')
  else
    phase('done')
    timer:stop()
    timer:close()
  end
end))
