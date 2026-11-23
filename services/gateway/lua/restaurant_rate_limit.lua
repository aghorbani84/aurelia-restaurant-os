local d = ngx.shared.auth_limits
local ip = ngx.var.binary_remote_addr or ngx.var.remote_addr or "unknown"
local key = "restaurant-apply:" .. ip
local n, err = d:incr(key, 1, 0, 60)
if not n then
    return ngx.exit(503)
end
if n == 1 then
    d:expire(key, 60)
end
if n > 10 then
    ngx.header["Retry-After"] = "60"
    return ngx.exit(429)
end
