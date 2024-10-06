# UmbrellaPortal
Simple Proxy Portal to allow HTTP Proxy and upstream traffic to socks5

/etc/umportal.json
```
{
    "http_portal": "192.168.*.*:8080",
    "upstreams": {
        "socks5": "192.168.*.*:1080"
    }
}
```

umportal --config /etc/umportal.json


