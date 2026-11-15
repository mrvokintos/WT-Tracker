#!/usr/bin/env python3
"""Mock War Thunder local API for testing the tracker without the game.

    python3 tools/mock_api.py [port]               # default 8111
    WT_API_URL=http://localhost:18111 wt-tracker   # for a non-default port

5 s in the hangar, then a battle with a new SCRIPT message every 2 s.
"""
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

SCRIPT = [
    "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)",
    "Joe (F-16C) сбил Petr (Ка-52)",
    "1:05 Bot ([ИИ] Панцирь-С1) сбил Joe (F-16C)",
    "Drone (MQ-1) уничтожил Hans2 (Leopard 2A7V)",
] + [f"Ace (Leopard 2A7V) уничтожил V{i} (Т-90М)" for i in range(9)] + [
    "Hans (Leopard 2A7V) разбился",
]
START = time.time()
HANGAR_SECS = 5

def battle_msgs():
    t = time.time() - START - HANGAR_SECS
    n = int(t // 2) + 1 if t >= 0 else 0
    return [{"id": i + 1, "msg": SCRIPT[i % len(SCRIPT)], "sender": "", "enemy": False, "mode": "", "time": i}
            for i in range(n)]

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _json(self, obj):
        b = json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        u = urlparse(self.path)
        in_battle = time.time() - START >= HANGAR_SECS
        if u.path == "/mission.json":
            self._json({"status": "running" if in_battle else "fail", "objectives": []})
        elif u.path == "/map_info.json":
            self._json({"valid": in_battle})
        elif u.path == "/hudmsg":
            last = int(parse_qs(u.query).get("lastDmg", ["0"])[0])
            self._json({"events": [], "damage": [m for m in battle_msgs() if m["id"] > last]})
        else:
            self.send_response(404); self.end_headers()

if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8111
    print(f"mock WT API на http://localhost:{port}")
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
