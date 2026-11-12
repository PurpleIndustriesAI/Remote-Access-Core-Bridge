import asyncio
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock, patch

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from backend import session_router as sr


def slot(key, title, *, allowed=True, remote=False, private=False):
    return SimpleNamespace(
        key=key,
        display_title=title,
        agent="codexcrew",
        folder_id="folder",
        workspace="default",
        running=False,
        _in_stage_execution=False,
        executor="remote" if remote else "",
        memory_mode="temporary" if private else "persistent",
        _app="",
        linked_session_key="",
        mode="",
        allowed=allowed,
    )


class FakeState:
    def __init__(self, slots):
        self._slots = {item.key: item for item in slots}

    def get_slot(self, key):
        return self._slots.get(key)


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.public = sr.public_hex(self.key)

    def test_signed_request_and_result(self):
        request = sr.make_request(self.key, "kimi", "list", {"cursor": 0})
        self.assertEqual(sr.verify_envelope(request, self.public), request)
        response = sr.make_response(self.key, request, "completed", {"sessions": []})
        self.assertEqual(sr.verify_envelope(response, self.public), response)
        self.assertEqual(response["request_id"], request["id"])

    def test_tampering_and_expiry_fail(self):
        request = sr.make_request(self.key, "codex", "list", {"cursor": 0})
        request["payload"]["cursor"] = 99
        with self.assertRaises(InvalidSignature):
            sr.verify_envelope(request, self.public)
        fresh = sr.make_request(self.key, "codex", "list", {"cursor": 0})
        with self.assertRaisesRegex(ValueError, "expired"):
            sr.verify_envelope(fresh, self.public, now=fresh["expires"] + 1)

    def test_private_file_round_trip(self):
        request = sr.make_request(self.key, "kimi", "list", {"cursor": 0})
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "request.json"
            sr.write_private(path, request)
            self.assertEqual(sr.read_envelope(path), request)

    def test_duplicate_peer_names_are_not_a_target(self):
        status = {"peers": [{"name": "Miku", "fingerprint": "a" * 64}, {"name": "Miku", "fingerprint": "b" * 64}]}
        with self.assertRaisesRegex(ValueError, "unique"):
            sr.peer_fingerprint(status, "Miku")
        self.assertEqual(sr.peer_fingerprint(status, "b" * 64), "b" * 64)


class RouterTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.public = sr.public_hex(self.key)
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def router(self, slots):
        return sr.SessionRouter(FakeState(slots), self.root)

    async def test_directory_pages_and_exclusions(self):
        slots = [slot(f"chat-{i:02d}", f"Chat {i:02d}") for i in range(12)]
        slots += [slot("private", "Private", private=True), slot("remote", "Remote", remote=True), slot("denied", "Denied", allowed=False)]
        state = FakeState(slots)
        router = self.router(slots)
        router.config_path.write_text(json.dumps({"enabled": True, "broker_session": "broker", "peer": "peer"}))

        def authorize(dashboard, *, caller_session_key, target, operation, exact_slot):
            candidate = dashboard.get_slot(target)
            if not candidate.allowed or candidate.memory_mode != "persistent":
                from codex_crew.dashboard.session_control import SessionControlError
                raise SessionControlError("denied")
            return candidate

        with patch("codex_crew.dashboard.session_control.authorize_target", side_effect=authorize):
            first = await router._list({"cursor": 0})
            second = await router._list({"cursor": first["next_cursor"]})
        self.assertEqual(first["total"], 12)
        self.assertEqual(len(first["sessions"]), 10)
        self.assertEqual(first["next_cursor"], 10)
        self.assertEqual(len(second["sessions"]), 2)
        self.assertIsNone(second["next_cursor"])
        keys = {item["key"] for item in first["sessions"] + second["sessions"]}
        self.assertNotIn("private", keys)
        self.assertNotIn("remote", keys)
        self.assertNotIn("denied", keys)

    async def test_send_uses_exact_target_and_provenance(self):
        router = self.router([])
        router.config_path.write_text(json.dumps({"enabled": True, "broker_session": "broker", "peer": "peer"}))
        request = sr.make_request(self.key, "codex", "send", {"target": "chat-exact", "text": "hello"})
        sent = AsyncMock(return_value={"target": "chat-exact", "started": False})
        with patch("codex_crew.dashboard.session_control.send_to_target", new=sent):
            result = await router._send(request, request["payload"])
        self.assertFalse(result["started"])
        kwargs = sent.await_args.kwargs
        self.assertEqual(kwargs["caller_session_key"], "broker")
        self.assertEqual(kwargs["target"], "chat-exact")
        self.assertTrue(kwargs["exact_slot"])
        self.assertIn("RAC codex request", kwargs["message"])
        self.assertTrue(kwargs["message"].endswith("hello"))

    async def test_reply_cursor_is_preserved(self):
        router = self.router([])
        router.config_path.write_text(json.dumps({"enabled": True, "broker_session": "broker", "peer": "peer"}))
        reply = {
            "target": "chat-exact", "title": "Exact", "running": False,
            "next_since": 9, "total": 9,
            "messages": [{"index": 8, "role": "assistant", "content": "safe", "ts": "now"}],
        }
        with patch("codex_crew.dashboard.session_control.read_messages", return_value=reply) as read:
            result = router._read({"target": "chat-exact", "since": 8})
        self.assertEqual(result["next_since"], 9)
        self.assertEqual(result["messages"][0]["content"], "safe")
        self.assertEqual(read.call_args.kwargs["since"], 8)

    async def test_duplicate_request_never_reexecutes_or_resends(self):
        router = self.router([])
        router.config_path.write_text(json.dumps({"enabled": True, "broker_session": "broker", "peer": "peer"}))
        request = sr.make_request(self.key, "kimi", "list", {"cursor": 0})
        path = self.root / ("crew-session-request-" + request["id"] + ".json")
        sr.write_private(path, request)
        response = sr.make_response(self.key, request, "completed", {"sessions": []})
        execute = AsyncMock(return_value=response)
        send = Mock(return_value={"ok": True})
        with (
            patch.object(sr, "crew_link_settings", return_value=self.public),
            patch.object(router, "_execute", new=execute),
            patch.object(sr.bridge, "call", new=send),
        ):
            await router._handle(path)
            await router._handle(path)
        self.assertEqual(execute.await_count, 1)
        self.assertEqual(send.call_count, 1)


if __name__ == "__main__":
    unittest.main()
