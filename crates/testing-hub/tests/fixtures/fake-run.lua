-- A stand-in for the engine's command line in a run from the app (tests/launch.rs): `run --root <root> [--file
-- <text>]` and `sheets run --task <id> --root <root>`, run as the hub runs the engine:
-- `nvim --headless -u NONE -i NONE -n -l fake-run.lua <args>`. It writes its arguments, one per line, and its working
-- folder into `<root>/argv.txt`, so a test can say exactly what the hub asked, and what it does is chosen by the file
-- text or the task:
--
--   (suite)         prints a line that names the root, ends green
--   red             prints a red case, exit 1
--   slow            waits until it is killed (writes `<root>/slow.started` first)
--   <task>          exit 0 for the task `alpha.nvim/try-it`, else the refusal of the engine, exit 2

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
  if value("--task") == "alpha.nvim/try-it" then
    io.stdout:write("sheet try-it: green\n")
    os.exit(0)
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
