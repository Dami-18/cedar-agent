-- benchmark/cedar.lua
--
-- Sysbench workload replay engine for cedar-agent.
--
-- Loads requests.jsonl once per thread, keeps everything in memory.
-- Each event() fires one HTTP POST and records success/failure.

-- cedar-agent exposes 2 routes on the same server:
--   POST /v1/is_authorized - stateless Cedar authorizer
--   POST /v1/is_authorized/poltree - PolTree cached authorizer

-- Sysbench reads these from the CLI as --<name>=<value>
-- "string" options  → --cedar-url=http://...
-- "bool"   options  → --random-access=on  or  --random-access=off

sysbench.cmdline.options = {
    ["cedar-url"] = {
        description = "Full authorization endpoint URL (including path)",
        type        = "string",
        default     = "http://127.0.0.1:8180/v1/is_authorized",
    },
    ["requests-file"] = {
        description = "Path to requests.jsonl produced by generate_bench_data",
        type        = "string",
        default     = "bench_data/requests.jsonl",
    },
    ["random-access"] = {
        description = "on = random request order, off = sequential (default)",
        type        = "bool",
        default     = false,   -- sysbench maps false → off
    },
}

-- Each sysbench worker thread is a separate Lua state, so these are
-- effectively thread-local even though they look like globals.

local _requests     = nil   -- array of raw JSON line strings, 1-indexed
local _n_requests   = 0
local _endpoint_url = nil
local _thread_cursor = 0    -- sequential position for this thread

local function load_requests(path)
    local lines = {}
    local f, err = io.open(path, "r")
    if not f then
        error("Cannot open requests file: " .. path .. " — " .. tostring(err))
    end
    for line in f:lines() do
        -- trim leading/trailing whitespace
        line = line:match("^%s*(.-)%s*$")
        if #line > 0 then
            lines[#lines + 1] = line
        end
    end
    f:close()
    if #lines == 0 then
        error("requests file is empty: " .. path)
    end
    return lines
end

-- sysbench.http.post(url, body, headers) → { status, body }
-- Available in sysbench >= 1.0 when compiled with HTTP support.
local function do_post(url, json_body)
    return sysbench.http.post(
        url,
        json_body,
        { ["Content-Type"] = "application/json" }
    )
end

function thread_init()
    local path      = sysbench.opt["requests-file"]
    _requests       = load_requests(path)
    _n_requests     = #_requests
    _endpoint_url   = sysbench.opt["cedar-url"]
    -- Stagger each thread's starting position so they don't all hit the same
    -- requests at the same time (sysbench.tid is 0-based).
    _thread_cursor  = sysbench.tid

    print(string.format("[thread %02d] loaded %d requests → %s",
        sysbench.tid, _n_requests, _endpoint_url))
end

-- event() is called once per benchmark iteration (i.e. per request).
function event()
    local idx

    if sysbench.opt["random-access"] then
        -- uniform random pick
        idx = math.random(1, _n_requests)
    else
        -- sequential, wrapping
        _thread_cursor = _thread_cursor + 1
        idx = ((_thread_cursor - 1) % _n_requests) + 1
    end

    local body = _requests[idx]
    local resp  = do_post(_endpoint_url, body)

    -- Any non-200 is recorded as an error by sysbench
    if resp.status ~= 200 then
        error(string.format("HTTP %d from cedar-agent (req idx=%d): %.200s",
            resp.status, idx, tostring(resp.body)))
    end
end

function thread_done()
    -- nothing to release; Lua GC handles _requests
end

function sysbench.hooks.report_cumulative(stat)
    io.write(string.format(
        "\n[cedar-agent] endpoint=%s | total=%d | QPS=%.0f | avg=%.2fms | p95=%.2fms | errors=%d\n",
        _endpoint_url,
        stat.events,
        stat.events / stat.time_total,
        stat.latency_avg * 1000,
        stat.latency_pct["95"] * 1000,
        stat.errors
    ))
end