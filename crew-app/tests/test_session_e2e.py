import asyncio
import json
import os
import socket
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from backend import bridge
from backend import session_router as sr


BINARY = Path(__file__).resolve().parents[2] / "target" / "release" / "remote-access-crew.exe"


def free_pair():
    while True:
        first = socket.socket()
        first.bind(("127.0.0.1", 0))
        port = first.getsockname()[1]
        first.close()
        second = socket.socket()
        try:
            second.bind(("127.0.0.1", port + 1))
        except OSError:
            continue
        finally:
            second.close()
        return port


def fake_slot(key, title):
    return SimpleNamespace(
        key=key, display_title=title, agent="codexcrew", folder_id="", workspace="default",
        running=False, _in_stage_execution=False, executor="", memory_mode="persistent",
        _app="", linked_session_key="", mode="",
    )


class FakeState:
    def __init__(self):
        self._slots = {key: fake_slot(key, key) for key in ("broker", "chat-test")}

    def get_slot(self, key):
        return self._slots.get(key)


class LiveSessionRouterTests(unittest.IsolatedAsyncioTestCase):
    async def test_two_real_bridges_directory_send_and_reply(self):
        if not BINARY.is_file():
            self.skipTest("release bridge is not built")
        processes = []
        homes = {}
        old_home = os.environ.get("RAC_HOME")
        try:
            with tempfile.TemporaryDirectory() as root:
                root = Path(root)
                for name in ("a", "b"):
                    home = root / name
                    home.mkdir()
                    port = free_pair()
                    homes[name] = (home, port)
                    (home / "state.json").write_text(json.dumps({
                        "peers": [],
                        "settings": {"port": port, "autosync": False, "channels": [sr.CHANNEL], "consent_revision": 1},
                    }))
                    processes.append(self._start(name, home))

                fingerprints = {name: self._status(home)["fingerprint"] for name, (home, _) in homes.items()}
                for process in processes:
                    process.terminate()
                for process in processes:
                    process.wait(timeout=10)
                processes.clear()

                for name, (home, port) in homes.items():
                    other = "b" if name == "a" else "a"
                    other_port = homes[other][1]
                    state = json.loads((home / "state.json").read_text())
                    state["peers"] = [{
                        "name": other,
                        "fingerprint": fingerprints[other],
                        "addrs": [f"127.0.0.1:{other_port}"],
                        "autosync": False,
                        "paired_at": 0,
                        "permissions": {
                            "clipboard": False,
                            "files": True,
                            "device_inventory": False,
                            "session_routing": True,
                        },
                    }]
                    (home / "state.json").write_text(json.dumps(state))
                    processes.append(self._start(name, home))
                for home, _ in homes.values():
                    status = self._status(home)
                    self.assertTrue(status["peers"][0]["permissions"]["files"], status)
                    self.assertTrue(status["peers"][0]["permissions"]["session_routing"], status)

                key_a = Ed25519PrivateKey.generate()
                key_b = Ed25519PrivateKey.generate()
                self.sender_public = sr.public_hex(key_a)
                state = FakeState()
                router = sr.SessionRouter(state, root / "router")
                sr.write_private(router.config_path, {
                    "enabled": True,
                    "broker_session": "broker",
                    "peer": fingerprints["a"],
                })

                def authorize(dashboard, *, caller_session_key, target, operation, exact_slot):
                    if target == "broker":
                        from codex_crew.dashboard.session_control import SessionControlError
                        raise SessionControlError("self target")
                    return dashboard.get_slot(target)

                async def list_page(cursor):
                    request = sr.make_request(key_a, "kimi", "list", {"cursor": cursor})
                    return await self._exchange(request, key_b, fingerprints, homes, router, authorize)

                page = await list_page(0)
                self.assertEqual(page["total"], 1)
                self.assertEqual(page["sessions"][0]["key"], "chat-test")
                self.assertIsNone(page["next_cursor"])

                sent = AsyncMock(return_value={"target": "chat-test", "started": True})
                send_request = sr.make_request(key_a, "codex", "send", {"target": "chat-test", "text": "loopback nonce"})
                with patch("codex_crew.dashboard.session_control.send_to_target", new=sent):
                    sent_result = await self._exchange(send_request, key_b, fingerprints, homes, router, authorize)
                self.assertTrue(sent_result["started"])
                self.assertEqual(sent.await_args.kwargs["target"], "chat-test")

                reply = {
                    "target": "chat-test", "title": "chat-test", "running": False,
                    "next_since": 1, "total": 1,
                    "messages": [{"index": 0, "role": "assistant", "content": "reply nonce", "ts": "now"}],
                }
                read_request = sr.make_request(key_a, "kimi", "read", {"target": "chat-test", "since": 0})
                with patch("codex_crew.dashboard.session_control.read_messages", return_value=reply):
                    read_result = await self._exchange(read_request, key_b, fingerprints, homes, router, authorize)
                self.assertEqual(read_result["messages"][0]["content"], "reply nonce")
                self.assertEqual(read_result["next_since"], 1)
        finally:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
            for process in processes:
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
            if old_home is None:
                os.environ.pop("RAC_HOME", None)
            else:
                os.environ["RAC_HOME"] = old_home

    def _start(self, name, home):
        env = dict(os.environ, RAC_HOME=str(home))
        flags = 0x08000000 if os.name == "nt" else 0
        process = subprocess.Popen(
            [str(BINARY), "--no-clipboard"],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=flags,
        )
        os.environ["RAC_HOME"] = str(home)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"bridge {name} exited")
            try:
                bridge.call("status", timeout=1)
                return process
            except Exception:
                time.sleep(0.1)
        raise RuntimeError(f"bridge {name} did not start")

    def _status(self, home):
        os.environ["RAC_HOME"] = str(home)
        return bridge.call("status", timeout=2)

    async def _exchange(self, request, host_key, fingerprints, homes, router, authorize):
        client_home = homes["a"][0]
        host_home = homes["b"][0]
        request_path = client_home / "outbox" / ("crew-session-request-" + request["id"] + ".json")
        sr.write_private(request_path, request)
        os.environ["RAC_HOME"] = str(client_home)
        await asyncio.to_thread(
            bridge.call,
            "channel_send",
            {"peer": fingerprints["b"], "channel": sr.CHANNEL, "path": str(request_path)},
            30,
        )
        incoming = host_home / "channels" / sr.CHANNEL / "inbox" / fingerprints["a"] / request_path.name
        deadline = time.monotonic() + 10
        while not incoming.exists():
            if time.monotonic() > deadline:
                raise TimeoutError("request did not arrive")
            await asyncio.sleep(0.05)
        os.environ["RAC_HOME"] = str(host_home)
        with (
            patch.object(sr, "crew_link_settings", return_value=self.sender_public),
            patch.object(sr, "local_signing_key", return_value=host_key),
            patch("codex_crew.dashboard.session_control.authorize_target", side_effect=authorize),
        ):
            await router._handle(incoming)

        os.environ["RAC_HOME"] = str(client_home)
        inbox = client_home / "channels" / sr.CHANNEL / "inbox" / fingerprints["b"]
        deadline = time.monotonic() + 10
        while True:
            for candidate in sorted(inbox.glob("crew-session-result-*.json")):
                response = sr.read_envelope(candidate)
                if response.get("request_id") != request["id"]:
                    continue
                sr.verify_envelope(response, sr.public_hex(host_key))
                self.assertEqual(response["payload"].get("outcome"), "completed")
                return response["payload"]
            if time.monotonic() > deadline:
                raise TimeoutError("signed response did not arrive")
            await asyncio.sleep(0.05)
