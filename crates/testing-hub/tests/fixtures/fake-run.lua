-- A stand-in for the engine's command line in a run from the app (tests/launch.rs): `run --root <root> [--file
-- <text>]` and `sheets run --task <id> --root <root>`, run as the hub runs the engine:
-- `nvim --headless -u NONE -i NONE -n -l fake-run.lua <args>`. It writes its arguments, one per line, and its working
-- folder into `<root>/argv.txt`, so a test can say exactly what the hub asked, and what it does is chosen by the file
-- text or the task:
--
--   (suite)         prints a line that names the root, ends green
--   red             prints a red case, exit 1
--   slow            waits until it is killed (writes `<root>/slow.started` first)
--   tree            starts a process of its own that writes into `<root>/beat.txt` while it lives, then waits
--                   until it is killed
--   orphan          leaves a process behind that holds its stdout and stderr for 25 s, and ends green
--   <task>          exit 0 for the task `alpha.nvim/try-it`, else the refusal of the engine, exit 2; with `--tasks`
--                   the task `alpha.nvim/other` has green sheets and is not written, `alpha.nvim/red` a red sheet
--                   and is written (both exit 1). With `--result` the last line is the engine's document of
--                   the run (`testing-sheet-run/1`), as the real command prints it

-- luacheck: globals vim

local argv = _G.arg or {}
local root
for i = 1, #argv do
  if argv[i] == "--root" then
    root = argv[i + 1]
  end
end
assert(root, "fake-run: --root")
local norm = root:gsub("\\", "/")

local f = assert(io.open(norm .. "/argv.txt", "w"))
for _, a in ipairs(argv) do
  f:write(a, "\n")
end
f:write("cwd=", (vim.uv.cwd() or ""):gsub("\\", "/"), "\n")
f:close()

local function value(name)
  for i = 1, #argv do
    if argv[i] == name then
      return argv[i + 1]
    end
  end
end

if argv[1] == "sheets" then
  assert(argv[2] == "run", "fake-run: sheets run")
  local vault, task = value("--tasks"), value("--task")
  local wants_result = vim.list_contains(argv, "--result")
  ---The lines of the real command, then (with `--result`) its document, then the exit code.
  local function done(exit, sheet, status, outcome, message)
    io.stdout:write(("sheet %s: %s\n"):format(sheet, status))
    if outcome then
      io.stdout:write(("testing sheets: task %s %s%s\n"):format(task, outcome, message and (": " .. message) or ""))
    end
    if wants_result then
      local written = vim.NIL
      if outcome then
        written = outcome == "changed"
      end
      io.stdout:write(vim.json.encode({
        schema = "testing-sheet-run/1",
        exit = exit,
        task = task,
        sheets = { { id = sheet, status = status } },
        red = status == "fail",
        tasks = outcome and { { id = task, outcome = outcome, message = message } } or {},
        written = written,
      }) .. "\n")
    end
    os.exit(exit)
  end
  if task == "alpha.nvim/try-it" then
    done(0, "try-it", "pass", vault and "changed")
  end
  if vault and task == "alpha.nvim/other" then
    -- Green sheets, and a task edited in between: the engine's exit 1 that is not a red sheet. The message names the
    -- vault, as a line of the engine may.
    done(1, "other", "pass", "failed", "conflict: " .. vault .. "/x.md changed")
  end
  if vault and task == "alpha.nvim/red" then
    -- The other exit 1: a red sheet, and the task says so.
    done(1, "red", "fail", "changed")
  end
  io.stderr:write("testing sheets run: no sheet names the task\n")
  os.exit(2)
end

assert(argv[1] == "run", "fake-run: run")
local file = value("--file")
if file == "red" then
  io.stdout:write(root .. "/TESTS/red_spec.lua:3: expected 1, got 2\n")
  io.stdout:write("RED 1 failed\n")
  os.exit(1)
elseif file == "orphan" then
  -- As a child editor that outlives its run does: it inherits the pipes and keeps them for a while.
  local handle = vim.uv.spawn(vim.v.progpath, {
    args = { "--headless", "-u", "NONE", "-i", "NONE", "-n", "--cmd", "lua vim.wait(25000)", "--cmd", "qa!" },
    stdio = { nil, 1, 2 },
    -- Not in the root: on Windows a folder in use as a working folder cannot be removed by the test.
    cwd = vim.uv.os_tmpdir(),
    detached = true,
  })
  assert(handle, "fake-run: no process to leave behind")
  handle:unref()
  io.stdout:write("GREEN 1 passed\n")
  io.stdout:flush()
  os.exit(0)
elseif file == "tree" then
  -- As a run whose children start processes of their own (a child editor's shell, `cargo test`): a process that goes
  -- on writing for as long as it lives. A minute at most, so that a test that fails leaves nothing behind for long.
  -- On Windows it is started through `cmd`: Neovim (libuv) ends its own children when it ends, but not what those
  -- started, so only a grandchild shows whether the whole tree is ended.
  local script = norm .. "/beat.lua"
  local b = assert(io.open(script, "w"))
  b:write(table.concat({
    "local path = _G.arg[1]",
    "for _ = 1, 600 do",
    '  local f = io.open(path, "a")',
    "  if f then",
    '    f:write("x")',
    "    f:close()",
    "  end",
    "  vim.uv.sleep(100)",
    "end",
    "",
  }, "\n"))
  b:close()
  local words = { "--headless", "-u", "NONE", "-i", "NONE", "-n", "-l", script, norm .. "/beat.txt" }
  local handle
  if vim.fn.has("win32") == 1 then
    -- `/s /c "<line>"`: cmd takes the line between the outer quotes as it is.
    local line = { '"' .. vim.v.progpath .. '"' }
    for _, word in ipairs(words) do
      line[#line + 1] = '"' .. word .. '"'
    end
    handle = vim.uv.spawn(vim.env.COMSPEC or "cmd.exe", {
      args = { "/d", "/s", "/c", '"' .. table.concat(line, " ") .. '"' },
      verbatim = true,
      -- Not in the root: on Windows a folder in use as a working folder cannot be removed by the test.
      cwd = vim.uv.os_tmpdir(),
    })
  else
    handle = vim.uv.spawn(vim.v.progpath, { args = words, cwd = vim.uv.os_tmpdir() })
  end
  assert(handle, "fake-run: no process of its own")
  while true do
    vim.uv.sleep(100)
  end
elseif file == "slow" then
  local s = assert(io.open(norm .. "/slow.started", "w"))
  s:close()
  while true do
    vim.uv.sleep(100)
  end
end
io.stdout:write("ran in " .. root .. "\n")
io.stdout:write("GREEN 3 passed\n")
os.exit(0)
