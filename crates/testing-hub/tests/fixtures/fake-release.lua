-- A stand-in for the engine's `testing ci release ... --json` (tests/release.rs), run as the hub runs the engine:
-- `nvim --headless -u NONE -i NONE -n -l fake-release.lua ci release <step args> --json`. It appends its arguments
-- (one line per call, joined by blanks) to `$FAKE_RELEASE_DIR/argv.txt`, and the names of the variables it was given
-- to `env.txt`, so a test can say exactly what the hub asked and with what. A release is written to `released.txt`:
-- the stand-in for the list of reviewed commits, which only this script (the engine) ever writes.
--
-- What it does besides is chosen by the first line of `$FAKE_RELEASE_DIR/mode.txt`:
--
--   (none)      the step's document
--   old         an engine without the step: its usage on stderr, nothing on stdout, exit 2
--   wait        the step's document, once `$FAKE_RELEASE_DIR/go` is there
--   moved       `show` answers for another commit than the one asked (the id named something else)
--   rerun-red   the release is recorded, a re-run fails: exit 3 with the document

-- luacheck: globals vim

local argv = _G.arg or {}
local dir = assert(os.getenv("FAKE_RELEASE_DIR"), "fake-release: FAKE_RELEASE_DIR"):gsub("\\", "/")

local function append(name, line)
  local f = assert(io.open(dir .. "/" .. name, "a"))
  f:write(line, "\n")
  f:close()
end

append("argv.txt", table.concat(argv, " "))
local seen = {}
for _, name in ipairs({ "TESTING_RUNNER_ROOT", "GH_TOKEN", "GITHUB_TOKEN", "TESTING_TRUST", "FAKE_RELEASE_DIR" }) do
  if os.getenv(name) then
    seen[#seen + 1] = name
  end
end
append("env.txt", table.concat(seen, " "))
append("cwd.txt", (vim.uv.cwd() or ""):gsub("\\", "/"))

local mode = ""
local m = io.open(dir .. "/mode.txt", "r")
if m then
  mode = (m:read("l") or ""):gsub("%s+$", "")
  m:close()
end

assert(argv[1] == "ci" and argv[2] == "release", "fake-release: ci release")
assert(argv[#argv] == "--json", "fake-release: --json comes last")

if mode == "old" then
  io.stderr:write("testing ci: unknown argument release\n")
  os.exit(2)
end
if mode == "wait" then
  while not vim.uv.fs_stat(dir .. "/go") do
    vim.uv.sleep(20)
  end
end

local SCHEMA = "testing-ci-release/1"
local S1 = ("1"):rep(40)
local S2 = ("2"):rep(40)

local function done(doc)
  doc.schema = SCHEMA
  -- the engine may say something first: only the last line is the document
  io.stdout:write("note: this line is not the document\n")
  io.stdout:write(vim.json.encode(doc), "\n")
  io.stdout:flush()
  os.exit(doc.exit)
end

local function has(flag)
  for _, a in ipairs(argv) do
    if a == flag then
      return true
    end
  end
  return false
end

if has("--repos") then
  done({
    step = "repos",
    exit = 0,
    repos = {
      { name = "fixture-owner/alpha.nvim", here = false, mode = "auto", reviewed = 2 },
      { name = "elsewhere/tool", here = false, mode = "always", reviewed = 1 },
    },
  })
end

local repo = argv[3]
if has("--commits") then
  if repo == "elsewhere/tool" then
    done({
      step = "commits",
      repo = repo,
      exit = 3,
      problem = "cannot list the commits of elsewhere/tool: HTTP 404",
    })
  end
  done({
    step = "commits",
    repo = repo,
    exit = 0,
    commits = {
      { sha = S2, title = "feat: the newest", author = "Alice", date = "2026-11-07T10:00:00Z", released = false },
      { sha = S1, title = "fix: the one before", author = "Bob", date = "2026-11-06T10:00:00Z", released = true },
      { sha = "abc1234", title = "not a full id", author = "?", date = "", released = false },
    },
  })
end

local sha = argv[4]
if has("--show") then
  if sha == ("e"):rep(40) then
    done({ step = "show", repo = repo, exit = 3, problem = repo .. " has no commit " .. sha .. ": HTTP 404" })
  end
  local full = mode == "moved" and ("9"):rep(40) or sha
  done({
    step = "show",
    repo = repo,
    sha = full,
    exit = 0,
    show = {
      sha = full,
      already = false,
      prev = S1,
      compared = true,
      status = "ahead",
      total_commits = 1,
      commits = { { sha = full, author = "Alice", title = "feat: the newest" } },
      files = {
        { path = "lua/alpha/init.lua", status = "modified" },
        { path = ".github/workflows/ci.yml", status = "modified", flag = "workflow" },
      },
      files_total = 2,
      truncated = false,
    },
  })
end

if has("--yes") then
  append("released.txt", repo .. " " .. sha)
  if mode == "rerun-red" then
    done({
      step = "release",
      repo = repo,
      sha = sha,
      exit = 3,
      problem = "re-run 71 failed: HTTP 403",
      release = {
        already = false,
        recorded = true,
        reruns = { { id = 71, name = "ci", was = "failure", ok = false, problem = "HTTP 403" } },
      },
    })
  end
  done({
    step = "release",
    repo = repo,
    sha = sha,
    exit = 0,
    release = {
      already = false,
      recorded = true,
      reruns = { { id = 71, name = "ci", was = "failure", ok = true } },
    },
  })
end

io.stderr:write("fake-release: no step in " .. table.concat(argv, " ") .. "\n")
os.exit(9)
