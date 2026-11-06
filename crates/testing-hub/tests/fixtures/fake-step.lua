-- A stand-in for `testing sheets step <sheet> --root <root>` (tests/sheetstep.rs): it speaks
-- `testing-sheet-session/1` on stdout and reads commands on stdin, as the engine does, and what it
-- does is chosen by the name of the sheet. Every command line it reads goes into `<root>/commands.txt`,
-- so a test can say exactly what reached the child's stdin. Run as the hub runs the engine:
-- `nvim --headless -u NONE -i NONE -n -l fake-step.lua sheets step <sheet> --root <root>`.
--
--   green    two steps of one size; the first leaves a picture, the second a screen; ends green
--   ask      one step that is a question; the answer decides
--   refused  no event, the refusal of the workstation on stderr, exit 2
--   hang     stops before the first step and waits for its stdin to close (writes started, ended)
--   deaf     stops before the first step and never reads its stdin; writes `alive` every 100 ms
--   chatty   after start, one line longer than the hub takes

-- luacheck: globals vim

local argv = _G.arg or {}
assert(argv[1] == "sheets" and argv[2] == "step", "fake-step: not `sheets step`")
local sheet, root = argv[3], nil
for i = 4, #argv do
	if argv[i] == "--root" then
		root = argv[i + 1]
	end
end
assert(sheet and root, "fake-step: a sheet and --root")
root = root:gsub("\\", "/")

local SCHEMA = "testing-sheet-session/1"

local function emit(event, fields)
	fields = fields or {}
	fields.schema, fields.event = SCHEMA, event
	io.stdout:write(vim.json.encode(fields), "\n")
	io.stdout:flush()
end

local function record(line)
	local f = assert(io.open(root .. "/commands.txt", "a"))
	f:write(line, "\n")
	f:close()
end

local function touch(name, text)
	local f = assert(io.open(root .. "/" .. name, "w"))
	f:write(text or "")
	f:close()
end

--- The next command line, recorded; nil at the end of stdin.
local function next_command()
	local line = io.stdin:read("*l")
	if line == nil then
		return nil
	end
	line = vim.trim(line)
	if line ~= "" then
		record(line)
	end
	return line
end

local SIZE = "80x24"
local STEPS = {
	{ n = 1, action = "keys", value = ":UI<CR>", note = "open the float" },
	{ n = 2, action = "expect", value = "vim.fn.winnr('$') == 2" },
}

local function start(steps)
	emit("start", {
		id = "fake." .. sheet,
		title = "Fake " .. sheet,
		engine = "rpc",
		path = root .. "/TESTS/sheets/" .. sheet .. ".sheet.lua",
		root = root,
		sizes = { { rows = 24, cols = 80 } },
		steps = steps,
		headed = false,
		delay_ms = 0,
		breaks = {},
	})
	emit("size", { size = SIZE, state = "open" })
end

if sheet == "refused" then
	io.stderr:write(
		"testing sheets: refused on the workstation (NVIM_MACHINE_ROLE=workstation): a sheet starts editors in "
			.. root
			.. "\n"
	)
	os.exit(2)
elseif sheet == "hang" then
	start(STEPS)
	touch("started")
	emit("paused", { size = SIZE, n = 1, reason = "start" })
	while true do
		local line = next_command()
		if line == nil or line == "abort" then
			break
		end
	end
	touch("ended")
	emit("end", { status = "aborted", ok = false, verdict = false, failures = {}, snapshots = {}, ms = 1 })
	os.exit(3)
elseif sheet == "deaf" then
	start(STEPS)
	touch("started")
	emit("paused", { size = SIZE, n = 1, reason = "start" })
	while true do
		touch("alive", tostring(vim.uv.hrtime()))
		vim.wait(100)
	end
elseif sheet == "chatty" then
	start(STEPS)
	io.stdout:write(string.rep("x", 2 * 1024 * 1024), "\n")
	io.stdout:flush()
	vim.wait(10000)
	os.exit(0)
elseif sheet == "ask" then
	start({ { n = 1, action = "ask", value = "Does it sound right?" } })
	emit("step-before", { size = SIZE, n = 1, total = 1, action = "ask", value = "Does it sound right?" })
	emit("ask", { size = SIZE, n = 1, text = "Does it sound right?" })
	local yes, note
	while true do
		local line = next_command()
		if line == nil or line == "abort" then
			emit("end", { status = "aborted", ok = false, verdict = false, failures = {}, snapshots = {}, ms = 1 })
			os.exit(3)
		end
		local word, rest = line:match("^answer%s+(%a+)%s*(.-)%s*$")
		if word == "yes" or word == "no" then
			yes, note = word == "yes", rest ~= "" and rest or nil
			break
		end
		emit("error", { message = "this step waits for: answer yes|no [text]" })
	end
	emit("step-after", {
		size = SIZE,
		n = 1,
		status = yes and "pass" or "fail",
		ms = 2,
		reason = (not yes) and (note or "answered no") or nil,
	})
	if not yes then
		emit("paused", { size = SIZE, n = 1, reason = "failed" })
		next_command()
	end
	emit("end", {
		status = yes and "pass" or "fail",
		ok = yes,
		verdict = true,
		failures = yes and {} or { "80x24 step 1: " .. (note or "answered no") },
		snapshots = {},
		ms = 3,
	})
	os.exit(yes and 0 or 1)
elseif sheet == "green" then
	start(STEPS)
	-- the picture of step 1: a real PNG of one pixel, in a folder of the run
	vim.fn.mkdir(root .. "/evidence", "p")
	local png = root .. "/evidence/step-1.png"
	local f = assert(io.open(png, "wb"))
	f:write(
		vim.base64.decode(
			"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
		)
	)
	f:close()
	local mode = "step"
	local n = 1
	while n <= #STEPS do
		if mode == "step" then
			emit("paused", { size = SIZE, n = n, reason = n == 1 and "start" or "step" })
			while true do
				local line = next_command()
				if line == nil or line == "abort" then
					emit(
						"end",
						{ status = "aborted", ok = false, verdict = false, failures = {}, snapshots = {}, ms = 1 }
					)
					os.exit(3)
				elseif line == "next" or line:match("^until%s+%d+$") then
					break
				elseif line == "continue" then
					mode = "continue"
					break
				elseif not (line:match("^break%s+%d+$") or line:match("^clear%s+%d+$")) then
					-- a breakpoint leaves the run paused; anything else is no command
					emit("error", { message = "unknown command: " .. line })
				end
			end
		end
		local step = STEPS[n]
		emit("step-before", {
			size = SIZE,
			n = n,
			total = #STEPS,
			action = step.action,
			value = step.value,
			note = step.note,
		})
		local after = { size = SIZE, n = n, status = "pass", ms = 5 }
		if n == 1 then
			after.image = png
		else
			after.screen = { "~", "float", "~" }
		end
		emit("step-after", after)
		n = n + 1
	end
	emit("end", { status = "pass", ok = true, verdict = true, failures = {}, snapshots = {}, ms = 12 })
	os.exit(0)
else
	io.stderr:write("testing sheets: no sheet " .. sheet .. " in " .. root .. "/TESTS/sheets\n")
	os.exit(2)
end
