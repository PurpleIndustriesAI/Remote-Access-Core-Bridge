"""Signed, owner-approved routing between RAC peers and ordinary Crew sessions."""
from __future__ import annotations

import asyncio
import hashlib
import json
import os
import re
import sqlite3
import time
import uuid
from contextlib import closing
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey

from . import bridge

CHANNEL = "crew-sessions"
DOMAIN = b"crew-rac/session-routing/v1\x00"
MAX_FILE = 65536
MAX_MESSAGE = 4000
PAGE_SIZE = 10
READ_SIZE = 8
ID = re.compile(r"[0-9a-f]{32}\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")
AGENTS = {"kimi", "codex"}
FIELDS = {"version", "id", "kind", "agent", "issued", "expires", "request_id", "payload", "signature"}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def crew_link_settings():
    from codex_crew.config.loader import config_dir

    path = Path(config_dir()) / "apps" / "crew-link" / "data" / "settings.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    public = value.get("peer_public", "")
    if not SHA.fullmatch(public):
        raise ValueError("Crew Link peer signing key is not pinned")
    return public


def local_signing_key():
    from codex_crew.config.loader import config_dir
    from codex_crew.secrets import SecretVault

    node = "windows" if os.name == "nt" else "linux"
    stored = SecretVault(config_dir()).get("CREW_KDE_LINK_" + node.upper() + "_ED25519")
    if stored is None:
        raise ValueError("Crew Link signing key is not initialized")
    return Ed25519PrivateKey.from_private_bytes(bytes.fromhex(stored.reveal()))


def public_hex(key):
    return key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()


def make_request(key, agent, action, payload, *, request_id=None):
    if agent not in AGENTS or action not in {"list", "send", "read"}:
        raise ValueError("invalid session request")
    now = int(time.time())
    value = {
        "version": 1,
        "id": request_id or uuid.uuid4().hex,
        "kind": "request",
        "agent": agent,
        "issued": now,
        "expires": now + 300,
        "request_id": "",
        "payload": dict(payload, action=action),
    }
    value["signature"] = key.sign(DOMAIN + canonical(value)).hex()
    verify_envelope(value, public_hex(key))
    return value


def make_response(key, request, outcome, payload):
    if outcome not in {"completed", "blocked"}:
        raise ValueError("invalid session outcome")
    value = {
        "version": 1,
        "id": uuid.uuid4().hex,
        "kind": "result",
        "agent": request["agent"],
        "issued": int(time.time()),
        "expires": 0,
        "request_id": request["id"],
        "payload": dict(payload, outcome=outcome),
    }
    value["signature"] = key.sign(DOMAIN + canonical(value)).hex()
    verify_envelope(value, public_hex(key))
    return value


def verify_envelope(value, peer_public, *, now=None):
    if not isinstance(value, dict) or set(value) != FIELDS:
        raise ValueError("invalid session envelope")
    if value["version"] != 1 or type(value["version"]) is not int:
        raise ValueError("invalid session version")
    if not ID.fullmatch(value["id"]) or value["kind"] not in {"request", "result"}:
        raise ValueError("invalid session identity")
    if value["agent"] not in AGENTS or not isinstance(value["payload"], dict):
        raise ValueError("invalid session agent or payload")
    if not isinstance(value["signature"], str) or not re.fullmatch(r"[0-9a-f]{128}", value["signature"]):
        raise ValueError("invalid session signature")
    if not SHA.fullmatch(peer_public):
        raise ValueError("invalid peer public key")
    signature = value["signature"]
    unsigned = {k: v for k, v in value.items() if k != "signature"}
    Ed25519PublicKey.from_public_bytes(bytes.fromhex(peer_public)).verify(
        bytes.fromhex(signature), DOMAIN + canonical(unsigned)
    )
    if not isinstance(value["issued"], int) or not isinstance(value["expires"], int):
        raise ValueError("invalid session times")
    if value["kind"] == "request":
        current = time.time() if now is None else now
        if value["request_id"] or not 0 < value["expires"] - value["issued"] <= 300:
            raise ValueError("invalid session request correlation")
        if not value["issued"] - 30 <= current <= value["expires"]:
            raise ValueError("expired session request")
        if value["payload"].get("action") not in {"list", "send", "read"}:
            raise ValueError("invalid session action")
    elif not ID.fullmatch(value["request_id"]) or value["expires"] != 0:
        raise ValueError("invalid session result correlation")
    if len(canonical(value)) > MAX_FILE:
        raise ValueError("session envelope is too large")
    return value


def read_envelope(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError("unsafe session file")
    before = path.lstat()
    if before.st_size > MAX_FILE or before.st_nlink != 1:
        raise ValueError("unsafe session file")
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(fd, "rb") as stream:
        opened = os.fstat(stream.fileno())
        if (before.st_ino, before.st_dev) != (opened.st_ino, opened.st_dev):
            raise ValueError("changed session file")
        raw = stream.read(MAX_FILE + 1)
        after = os.fstat(stream.fileno())
    if len(raw) > MAX_FILE or (opened.st_size, opened.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
        raise ValueError("incomplete session file")
    return json.loads(raw.decode("utf-8"))


def write_private(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = canonical(value)
    temp = path.with_name(path.name + ".new")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    fd = os.open(temp, flags, 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)
    finally:
        if temp.exists():
            temp.unlink()


def peer_fingerprint(status, peer):
    peers = status.get("peers", [])
    matches = [
        candidate for candidate in peers
        if candidate.get("fingerprint") == peer or candidate.get("name", "").lower() == str(peer).lower()
    ]
    if not peer and len(peers) == 1:
        matches = peers
    if len(matches) != 1:
        raise ValueError("a unique paired peer is required")
    return matches[0]["fingerprint"]


class SessionRouter:
    def __init__(self, dashboard, data_dir):
        self.dashboard = dashboard
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.config_path = self.data_dir / "session_router.json"
        self.db_path = self.data_dir / "session_router.sqlite"
        self.task = None
        self.last_error = ""
        self._init_db()

    def _init_db(self):
        with closing(sqlite3.connect(self.db_path)) as db, db:
            db.execute(
                "CREATE TABLE IF NOT EXISTS requests ("
                "id TEXT PRIMARY KEY, sha TEXT NOT NULL, response TEXT NOT NULL, "
                "delivery_attempted INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL)"
            )

    def config(self):
        if not self.config_path.exists():
            return {"enabled": False, "broker_session": "", "peer": ""}
        value = json.loads(self.config_path.read_text(encoding="utf-8"))
        if not isinstance(value, dict) or set(value) != {"enabled", "broker_session", "peer"}:
            raise ValueError("invalid session router configuration")
        return value

    def _broker_candidates(self):
        result = []
        for slot in self.dashboard._slots.values():
            if (
                getattr(slot, "memory_mode", "persistent") == "persistent"
                and not getattr(slot, "_app", "")
                and not getattr(slot, "linked_session_key", "")
                and getattr(slot, "mode", "") != "crew"
                and getattr(slot, "executor", "") != "remote"
            ):
                result.append({"key": slot.key, "title": str(slot.display_title)})
        return sorted(result, key=lambda item: item["title"].lower())

    def snapshot(self):
        value = self.config()
        return {
            **value,
            "running": self.task is not None and not self.task.done(),
            "error": self.last_error,
            "broker_candidates": self._broker_candidates(),
        }

    async def configure(self, body):
        if not isinstance(body, dict) or set(body) - {"enabled", "broker_session", "peer"}:
            raise ValueError("invalid session router settings")
        value = {
            "enabled": body.get("enabled") is True,
            "broker_session": str(body.get("broker_session", "")).strip(),
            "peer": str(body.get("peer", "")).strip(),
        }
        if value["enabled"]:
            await self._validate(value)
        write_private(self.config_path, value)
        await self.stop()
        self.last_error = ""
        if value["enabled"]:
            await self.start()
        return self.snapshot()

    async def _validate(self, value):
        from codex_crew.dashboard.session_control import caller_slot_key

        broker = caller_slot_key(self.dashboard, value["broker_session"])
        slot = self.dashboard.get_slot(broker)
        if not broker or slot is None:
            raise ValueError("router chat is not open")
        if (
            getattr(slot, "memory_mode", "persistent") != "persistent"
            or getattr(slot, "_app", "")
            or getattr(slot, "linked_session_key", "")
            or getattr(slot, "mode", "") == "crew"
            or getattr(slot, "executor", "") == "remote"
        ):
            raise ValueError("router chat is not eligible")
        value["broker_session"] = broker
        status = await asyncio.to_thread(bridge.call, "status")
        fingerprint = peer_fingerprint(status, value["peer"])
        peer = next(p for p in status["peers"] if p["fingerprint"] == fingerprint)
        permissions = peer.get("permissions", {})
        if not permissions.get("files") or not permissions.get("session_routing"):
            raise ValueError("peer file and session routing approval are required")
        if CHANNEL not in status.get("settings", {}).get("channels", []):
            raise ValueError("the crew-sessions channel is not enabled")
        value["peer"] = fingerprint
        crew_link_settings()
        local_signing_key()

    async def start(self):
        if self.task is not None and not self.task.done():
            return
        value = self.config()
        if not value["enabled"]:
            return
        try:
            await self._validate(value)
            self.task = asyncio.create_task(self._run())
            self.last_error = ""
        except Exception as exc:  # configuration errors are safe status only
            self.last_error = str(exc)
            self.task = None

    async def stop(self):
        if self.task is not None:
            self.task.cancel()
            try:
                await self.task
            except asyncio.CancelledError:
                pass
            self.task = None

    async def _run(self):
        while True:
            try:
                await self.tick()
                self.last_error = ""
            except asyncio.CancelledError:
                raise
            except Exception as exc:  # keep failures visible without content
                self.last_error = type(exc).__name__
            await asyncio.sleep(2)

    def _inbox(self):
        value = self.config()
        return bridge.home() / "channels" / CHANNEL / "inbox" / value["peer"]

    async def tick(self):
        value = self.config()
        if not value["enabled"]:
            return
        await self._validate(value)
        inbox = self._inbox()
        if not inbox.is_dir():
            return
        files = sorted(inbox.glob("crew-session-request-*.json"))[:128]
        for path in files:
            await self._handle(path)

    async def _handle(self, path):
        request = read_envelope(path)
        peer_public = await asyncio.to_thread(crew_link_settings)
        verify_envelope(request, peer_public)
        sha = digest(request)
        with closing(sqlite3.connect(self.db_path)) as db, db:
            row = db.execute("SELECT sha,response,delivery_attempted FROM requests WHERE id=?", (request["id"],)).fetchone()
            if row and row[0] != sha:
                return
            if row and row[2]:
                return
            if row:
                response = json.loads(row[1])
            else:
                response = await self._execute(request)
                db.execute(
                    "INSERT INTO requests(id,sha,response,delivery_attempted,created) VALUES(?,?,?,0,?)",
                    (request["id"], sha, canonical(response).decode("ascii"), int(time.time())),
                )
            # One delivery attempt only. An uncertain result is never retried automatically.
            db.execute("UPDATE requests SET delivery_attempted=1 WHERE id=?", (request["id"],))
        outbox = self.data_dir / "outbox"
        target = outbox / ("crew-session-result-" + response["id"] + ".json")
        if not target.exists():
            write_private(target, response)
        peer = self.config()["peer"]
        await asyncio.to_thread(bridge.call, "channel_send", {"peer": peer, "channel": CHANNEL, "path": str(target)}, 60)

    async def _execute(self, request):
        key = await asyncio.to_thread(local_signing_key)
        try:
            payload = request["payload"]
            action = payload["action"]
            if action == "list":
                result = await self._list(payload)
            elif action == "send":
                result = await self._send(request, payload)
            else:
                result = self._read(payload)
            return make_response(key, request, "completed", result)
        except Exception as exc:
            code = getattr(exc, "code", "request_blocked")
            return make_response(key, request, "blocked", {"error": str(code)[:120]})

    async def _list(self, payload):
        from codex_crew.dashboard.session_control import SessionControlError, authorize_target, sanitize_outbound

        cursor = payload.get("cursor", 0)
        if type(cursor) is not int or cursor < 0:
            raise ValueError("invalid directory cursor")
        broker = self.config()["broker_session"]
        sessions = []
        for slot in sorted(self.dashboard._slots.values(), key=lambda item: item.key):
            if getattr(slot, "executor", "") == "remote":
                continue
            can_send = can_read = False
            try:
                authorize_target(self.dashboard, caller_session_key=broker, target=slot.key, operation="send", exact_slot=True)
                can_send = True
            except SessionControlError:
                pass
            try:
                authorize_target(self.dashboard, caller_session_key=broker, target=slot.key, operation="read", exact_slot=True)
                can_read = True
            except SessionControlError:
                pass
            if not (can_send or can_read):
                continue
            sessions.append({
                "key": slot.key,
                "title": sanitize_outbound(str(slot.display_title)),
                "agent": str(getattr(slot, "agent", "")),
                "folder": str(getattr(slot, "folder_id", "") or ""),
                "workspace": str(getattr(slot, "workspace", "default")),
                "running": bool(slot.running or getattr(slot, "_in_stage_execution", False)),
                "can_send": can_send,
                "can_read": can_read,
            })
        page = sessions[cursor:cursor + PAGE_SIZE]
        next_cursor = cursor + len(page) if cursor + len(page) < len(sessions) else None
        return {"sessions": page, "next_cursor": next_cursor, "total": len(sessions)}

    async def _send(self, request, payload):
        from codex_crew.dashboard.session_control import send_to_target

        target = payload.get("target", "")
        text = payload.get("text", "")
        if not isinstance(target, str) or not target or not isinstance(text, str) or not text.strip():
            raise ValueError("exact target and message are required")
        if len(text.encode("utf-8")) > MAX_MESSAGE:
            raise ValueError("message is too large")
        provenance = "RAC " + request["agent"] + " request " + request["id"] + "\n"
        result = await send_to_target(
            self.dashboard,
            caller_session_key=self.config()["broker_session"],
            target=target,
            message=provenance + text,
            exact_slot=True,
        )
        return {"target": result["target"], "started": result["started"]}

    def _read(self, payload):
        from codex_crew.dashboard.session_control import read_messages

        target = payload.get("target", "")
        since = payload.get("since")
        if not isinstance(target, str) or not target or (since is not None and (type(since) is not int or since < 0)):
            raise ValueError("invalid reply read")
        result = read_messages(
            self.dashboard,
            caller_session_key=self.config()["broker_session"],
            target=target,
            limit=READ_SIZE,
            since=since,
        )
        return {
            "target": result["target"],
            "title": result["title"],
            "running": result["running"],
            "next_since": result["next_since"],
            "total": result["total"],
            "messages": result["messages"],
        }
