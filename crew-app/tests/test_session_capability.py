import unittest
from pathlib import Path

from codex_crew.apps.context import build_app_context


class SessionRoutingCapabilityTests(unittest.TestCase):
    def test_dashboard_state_requires_explicit_capability(self):
        marker = object()
        root = Path(__file__).parent
        for permissions in ({}, {"sessionRouting": False}, {"storage": True}):
            with self.subTest(permissions=permissions):
                ctx = build_app_context(
                    "ordinary-app", root, permissions=permissions, dashboard_state=marker
                )
                self.assertIsNone(ctx.dashboard)

    def test_scoped_app_receives_only_the_injected_state(self):
        marker = object()
        ctx = build_app_context(
            "remote-access-crew",
            Path(__file__).parent,
            permissions={"sessionRouting": True},
            dashboard_state=marker,
        )
        self.assertIs(ctx.dashboard, marker)


if __name__ == "__main__":
    unittest.main()
