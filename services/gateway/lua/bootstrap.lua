local version = jit and jit.version or "LuaJIT unavailable"
ngx.log(ngx.NOTICE, "AURELIA gateway initialized with ", version)
