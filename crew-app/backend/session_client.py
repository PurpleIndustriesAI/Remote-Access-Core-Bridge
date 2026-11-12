"""Kimi and Codex client for selecting and messaging Crew sessions over RAC."""
from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

try:
    from . import bridge
    from .session_router import (
        CHANNEL,
        local_signing_key,
        make_request,
        peer_fingerprint,
        read_envelope,
        verify_envelope,
        crew_link_settings,
        write_private,
    )
except ImportError:
    import bridge
    from session_router import (
        CHANNEL,
        local_signing_key,
        make_request,
        peer_fingerprint,
        read_envelope,
        verify_envelope,
        crew_link_settings,
        write_private,
    )


def exchange(args, action, payload):
    status = bridge.call("status")
    fingerprint = peer_fingerprint(status, args.peer)
    peer = next(p for p in status["peers"] if p["fingerprint"] == fingerprint)
    permissions = peer.get("permissions", {})
    if not permissions.get("files") or not permissions.get("session_routing"):
        raise ValueError("local file and session routing approval are required for this peer")
    if CHANNEL not in status.get("settings", {}).get("channels", []):
        raise ValueError("enable the crew-sessions channel on this machine")
    key = local_signing_key()
    request = make_request(key, args.agent, action, payload)
    outbox = Path(args.data_dir) / "outbox"
    path = outbox / ("crew-session-request-" + request["id"] + ".json")
    if path.exists():
        raise ValueError("request already exists; use a new request id")
    write_private(path, request)
    bridge.call("channel_send", {"peer": fingerprint, "channel": CHANNEL, "path": str(path)}, 60)
    peer_public = crew_link_settings()
    inbox = bridge.home() / "channels" / CHANNEL / "inbox" / fingerprint
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        if inbox.is_dir():
            for candidate in sorted(inbox.glob("crew-session-result-*.json")):
                try:
                    response = read_envelope(candidate)
                except (ValueError, json.JSONDecodeError):
                    continue
                if response.get("request_id") != request["id"]:
                    continue
                verify_envelope(response, peer_public)
                if response["payload"].get("outcome") == "blocked":
                    raise ValueError("remote session request blocked: " + str(response["payload"].get("error", "")))
                if response["payload"].get("outcome") != "completed":
                    raise ValueError("invalid remote session outcome")
                return response["payload"]
        time.sleep(0.5)
    raise TimeoutError("no signed session response arrived; do not retry the same request automatically")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("list", "send", "read"))
    parser.add_argument("--peer", required=True, help="Exact RAC peer name or fingerprint")
    parser.add_argument("--agent", required=True, choices=("kimi", "codex"))
    parser.add_argument("--target", help="Exact session key from the signed directory")
    parser.add_argument("--text", help="Message for the selected session")
    parser.add_argument("--cursor", type=int, default=0)
    parser.add_argument("--since", type=int)
    parser.add_argument("--all", action="store_true", help="Read every directory page")
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--data-dir", default=str(Path(__file__).resolve().parent.parent / "data"))
    args = parser.parse_args()
    if not 1 <= args.timeout <= 120:
        parser.error("timeout must be between 1 and 120 seconds")
    if args.action == "list":
        if args.all:
            sessions = []
            cursor = 0
            total = None
            while True:
                page = exchange(args, "list", {"cursor": cursor})
                sessions.extend(page["sessions"])
                total = page["total"]
                if page["next_cursor"] is None:
                    break
                cursor = page["next_cursor"]
            unique = {item["key"]: item for item in sessions}
            result = {"sessions": list(unique.values()), "total": total, "outcome": "completed"}
        else:
            result = exchange(args, "list", {"cursor": args.cursor})
    elif args.action == "send":
        if not args.target or not args.text:
            parser.error("send requires --target and --text")
        result = exchange(args, "send", {"target": args.target, "text": args.text})
    else:
        if not args.target:
            parser.error("read requires --target")
        result = exchange(args, "read", {"target": args.target, "since": args.since})
    print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        raise SystemExit(str(exc)) from None
