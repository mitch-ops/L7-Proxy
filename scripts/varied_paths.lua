-- wrk Lua script that varies request paths for consistent hash testing
counter = 0

request = function()
    counter = counter + 1
    return wrk.format("GET", "/api/item/" .. (counter % 1000))
end
