local requests_file = os.getenv("CEDAR_REQUESTS_FILE") or "bench_data/requests.jsonl"

local _requests = {}
local _n_requests = 0
local _cursor = 0

local function load_requests(path)
   local lines = {}
   local f = io.open(path, "r")
   if not f then
      error("Cannot open requests file: " .. path)
   end
   for line in f:lines() do
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

function init(args)
   _requests = load_requests(requests_file)
   _n_requests = #_requests
   math.randomseed(os.time() + (tonumber(os.clock() * 1000000) or 0))
   _cursor = math.random(1, _n_requests)

   wrk.method = "POST"
   wrk.headers["Content-Type"] = "application/json"

   print(string.format("loaded %d requests from %s", _n_requests, requests_file))
end

function request()
   _cursor = _cursor + 1
   local body = _requests[((_cursor - 1) % _n_requests) + 1]
   return wrk.format("POST", nil, nil, body)
end

function response(status, headers, body)
   if status ~= 200 then
      io.stderr:write(string.format("HTTP %d: %.200s\n", status, body))
   end
end

function done(summary, latency, requests)
   io.write(string.format(
      "\n[cedar-agent summary]\n" ..
      "  requests : %d\n" ..
      "  errors   : connect=%d read=%d write=%d status=%d timeout=%d\n" ..
      "  avg      : %.3f ms\n" ..
      "  p50      : %.3f ms\n" ..
      "  p95      : %.3f ms\n" ..
      "  p99      : %.3f ms\n" ..
      "  max      : %.3f ms\n",
      summary.requests,
      summary.errors.connect, summary.errors.read, summary.errors.write,
      summary.errors.status, summary.errors.timeout,
      latency.mean / 1000,
      latency:percentile(50.0) / 1000,
      latency:percentile(95.0) / 1000,
      latency:percentile(99.0) / 1000,
      latency.max / 1000
   ))
end