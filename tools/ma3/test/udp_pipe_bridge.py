#!/usr/bin/env python3
"""Puts pipe_console.lua (the real surface plugin under stock Lua) behind a real UDP port so the
Rust service can be run against it end to end without onPC or LuaSocket. Standard library only.

    python3 tools/ma3/test/udp_pipe_bridge.py --port 9810 --plugin-arg "key=<hex> input=fake bench"

Stops after --seconds, or on SIGINT. Prints the plugin's log lines prefixed with "plugin:".
"""
import argparse
import os
import selectors
import socket
import subprocess
import sys
import time


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=9810)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--plugin-arg", required=True)
    ap.add_argument("--seconds", type=float, default=0, help="0 = run until interrupted")
    ap.add_argument("--frame-ms", type=float, default=16.0, help="console frame period simulated for the plugin loop")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    here = os.path.dirname(os.path.abspath(__file__))
    lua = subprocess.Popen(
        ["lua", os.path.join(here, "pipe_console.lua"), args.plugin_arg],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
    )
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((args.bind, args.port))
    sock.setblocking(False)
    sel = selectors.DefaultSelector()
    sel.register(sock, selectors.EVENT_READ)
    sel.register(lua.stdout, selectors.EVENT_READ)

    start = time.monotonic()
    next_tick = start
    pending_lines = []
    outputs = 0
    datagrams = 0
    try:
        while True:
            now = time.monotonic()
            if args.seconds and now - start > args.seconds:
                break
            timeout = max(0.0, next_tick - now)
            for key, _ in sel.select(timeout):
                if key.fileobj is sock:
                    while True:
                        try:
                            data, (ip, port) = sock.recvfrom(4096)
                        except BlockingIOError:
                            break
                        datagrams += 1
                        lua.stdin.write(f"D {ip} {port} {data.hex()}\n")
                else:
                    line = lua.stdout.readline()
                    if not line:
                        print("plugin: exited", flush=True)
                        return
                    line = line.rstrip("\n")
                    if line.startswith("S "):
                        _, ip, port, hexd = line.split(" ", 3)
                        try:
                            sock.sendto(bytes.fromhex(hexd), (ip, int(port)))
                            outputs += 1
                        except OSError as e:
                            print(f"bridge: send to {ip}:{port} failed: {e}", flush=True)
                    elif line.startswith("L ") and not args.quiet:
                        print("plugin: " + line[2:], flush=True)
            now = time.monotonic()
            if now >= next_tick:
                lua.stdin.write(f"T {now - start:.6f}\n")
                lua.stdin.flush()
                next_tick = now + args.frame_ms / 1000.0
    except KeyboardInterrupt:
        pass
    finally:
        try:
            lua.stdin.write("Q\n")
            lua.stdin.flush()
        except Exception:
            pass
        lua.wait(timeout=2)
        print(f"bridge: {datagrams} datagram(s) in, {outputs} out", flush=True)


if __name__ == "__main__":
    main()
