#!/usr/bin/env python3
"""Print OSC messages arriving on a UDP port. Standard library only.

    python3 tools/osc_listen.py 8000
"""
import socket
import struct
import sys


def read_padded_string(data, pos):
    end = data.index(b"\0", pos)
    s = data[pos:end].decode("utf-8", "replace")
    pos = end + 1
    pos += (-pos) % 4
    return s, pos


def decode(data):
    addr, pos = read_padded_string(data, 0)
    if pos >= len(data) or data[pos : pos + 1] != b",":
        return addr, []
    tags, pos = read_padded_string(data, pos)
    args = []
    for tag in tags[1:]:
        if tag == "i":
            args.append(struct.unpack(">i", data[pos : pos + 4])[0])
            pos += 4
        elif tag == "f":
            args.append(struct.unpack(">f", data[pos : pos + 4])[0])
            pos += 4
        elif tag == "s":
            s, pos = read_padded_string(data, pos)
            args.append(s)
        elif tag in "TFN":
            args.append({"T": True, "F": False, "N": None}[tag])
        else:
            args.append("<%s?>" % tag)
            break
    return addr, args


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", port))
    print("listening on udp/%d" % port, flush=True)
    while True:
        data, (host, _) = sock.recvfrom(2048)
        try:
            addr, args = decode(data)
            print("%s  %s %s" % (host, addr, " ".join(repr(a) for a in args)), flush=True)
        except Exception as e:  # noqa: BLE001
            print("%s  undecodable %r (%s)" % (host, data, e), flush=True)


if __name__ == "__main__":
    main()
