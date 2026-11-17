local dict = ngx.shared.auth_limits
local key = ngx.var.binary_remote_addr or ngx.var.remote_addr or "unknown"
local count, err = dict:incr(key, 1, 0, 60)

if not count then
    ngx.log(ngx.ERR, "auth rate limit counter unavailable: ", err)
    return ngx.exit(ngx.HTTP_SERVICE_UNAVAILABLE)
end

if count == 1 then
    dict:expire(key, 60)
end

if count > 10 then
    ngx.header["Retry-After"] = "60"
    return ngx.exit(ngx.HTTP_TOO_MANY_REQUESTS)
end
