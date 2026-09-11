-- Real Snacks Grep with repeated queries, then result-list movement.
-- Loaded into nvim --clean; no user init.lua or background plugins are run.
vim.o.swapfile = false
vim.o.shadafile = 'NONE'
vim.o.termguicolors = true
vim.o.guicursor = 'a:block-blinkon0'
vim.opt.rtp:append(assert(os.getenv('PERF_SNACKS_RTP'), 'set PERF_SNACKS_RTP to the installed snacks.nvim directory'))
require('snacks').setup({ picker = { enabled = true } })
local function phase(name)
  vim.fn.writefile({name, tostring(vim.o.columns), tostring(vim.o.lines)}, os.getenv('PERF_PHASE'))
end
phase('warmup')
vim.defer_fn(function()
  local picker = Snacks.picker.grep({cwd=os.getenv('PERF_GREP_ROOT') or vim.fn.getcwd(), search='term', layout={preset='default', fullscreen=true}})
  vim.defer_fn(function()
    local tick = 0
    local timer = vim.uv.new_timer()
    phase('snacks-grep')
    timer:start(0, 100, vim.schedule_wrap(function()
      tick = tick + 1
      if tick <= 80 then
        local queries = {'term','state','view','paint','self','fn','let','pub'}
        picker.input:set('', queries[(tick % #queries)+1])
        picker:find()
      elseif tick == 81 then
        phase('snacks-scroll')
      elseif tick <= 160 then
        picker.list:move(3)
      elseif tick == 161 then
        phase('idle')
      elseif tick > 200 then
        phase('done'); timer:stop(); timer:close()
      end
    end))
  end, 2000)
end, 3000)
