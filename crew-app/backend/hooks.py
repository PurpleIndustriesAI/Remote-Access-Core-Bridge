"""Crew lifecycle and owner-only routes for Remote Access Core (Bridge)."""
import asyncio
import json
from pathlib import Path

from aiohttp import web
from codex_crew.apps.route_registry import AppRoute

from . import bridge
from . import wizard
from .session_router import SessionRouter

SMALL = 16 * 1024
LARGE = 160 * 1024 * 1024  # a pasted screenshot as base64
_data_dir = None
_router = None


async def owner_gate(request, operation):
    from codex_crew.dashboard.handlers._shared import _is_restricted_session, require_owner_dashboard_request
    denied = await require_owner_dashboard_request(request, operation)
    if denied is not None:
        return denied
    if _is_restricted_session(request.app['state'], request):
        return web.json_response({'error': 'unrestricted owner session required'}, status=403)
    return None


async def on_startup(ctx):
    global _data_dir, _router
    _data_dir = Path(ctx.data_dir)
    await asyncio.to_thread(bridge.ensure_running, _data_dir)
    dashboard = getattr(ctx, 'dashboard', None)
    if dashboard is not None:
        _router = SessionRouter(dashboard, _data_dir)
        await _router.start()


async def on_shutdown(ctx):
    # The bridge keeps running so clipboard sync works while Crew is closed.
    if _router is not None:
        await _router.stop()


def routes(ctx):
    async def router_for(request):
        global _router
        if _router is None:
            _router = SessionRouter(request.app['state'], _data_dir or Path(ctx.data_dir))
            await _router.start()
        return _router

    def operation(cmd):
        async def handle(request, context):
            denied = await owner_gate(request, 'remote_access_crew.' + cmd)
            if denied is not None:
                return denied
            try:
                body = {}
                if request.method == 'POST':
                    limit = LARGE if cmd == 'send' else SMALL
                    if request.content_length is None or request.content_length > limit:
                        return web.json_response({'error': 'bounded JSON body required'}, status=413)
                    raw = await request.read()
                    if len(raw) > limit:
                        return web.json_response({'error': 'request too large'}, status=413)
                    body = json.loads(raw or b'{}')
                    if not isinstance(body, dict):
                        raise ValueError('JSON object required')
                router = await router_for(request) if cmd in {'status', 'session_router'} else None
                if cmd == 'status':
                    state = await asyncio.to_thread(bridge.ensure_running, _data_dir or Path(ctx.data_dir))
                    if state == 'missing':
                        return web.json_response({'bridge': 'missing'}, headers={'Cache-Control': 'no-store'})
                if cmd == 'session_router':
                    value = await router.configure(body) if request.method == 'POST' else router.snapshot()
                else:
                    value = await asyncio.to_thread(bridge.call, cmd, body, 600 if cmd == 'send' else 60)
                    if cmd == 'status':
                        value['session_router'] = router.snapshot()
                return web.json_response({'bridge': 'running', 'result': value}, headers={'Cache-Control': 'no-store'})
            except bridge.BridgeError as exc:
                return web.json_response({'error': str(exc)}, status=502)
            except (ValueError, KeyError) as exc:
                return web.json_response({'error': 'Invalid request: ' + str(exc)}, status=400)
            except Exception as exc:  # noqa: BLE001 - report a type, never internals
                return web.json_response({'error': 'Local operation unavailable: ' + type(exc).__name__}, status=503)
        return handle

    async def host_link(request, context):
        denied = await owner_gate(request, 'remote_access_crew.host_link')
        if denied is not None:
            return denied
        try:
            raw = await request.read()
            if len(raw) > SMALL:
                return web.json_response({'error': 'request too large'}, status=413)
            body = json.loads(raw or b'{}')
            if not isinstance(body, dict) or set(body) - {'remove'} or not isinstance(body.get('remove', False), bool):
                raise ValueError('expected {"remove": bool}')
            message = await asyncio.to_thread(bridge.host_link_setup, body.get('remove', False))
            return web.json_response({'result': {'message': message}}, headers={'Cache-Control': 'no-store'})
        except bridge.BridgeError as exc:
            return web.json_response({'error': str(exc)}, status=502)
        except ValueError as exc:
            return web.json_response({'error': 'Invalid request: ' + str(exc)}, status=400)

    async def wizard_check(request, context):
        """Read-only host verification for the setup wizard. Owner session required."""
        denied = await owner_gate(request, 'remote_access_crew.wizard_check')
        if denied is not None:
            return denied
        try:
            value = await asyncio.to_thread(wizard.checks)
            return web.json_response({'result': value}, headers={'Cache-Control': 'no-store'})
        except Exception as exc:  # noqa: BLE001 - report a type, never internals
            return web.json_response({'error': 'Local checks unavailable: ' + type(exc).__name__}, status=503)

    async def wizard_notes(request, context):
        """Compose handoff notes from live state. Owner session required."""
        denied = await owner_gate(request, 'remote_access_crew.wizard_notes')
        if denied is not None:
            return denied
        try:
            raw = await request.read()
            if len(raw) > SMALL:
                return web.json_response({'error': 'request too large'}, status=413)
            body = json.loads(raw or b'{}')
            if not isinstance(body, dict) or set(body) - {'status'}:
                raise ValueError('expected {} or {"status": bridge status dict}')
            text = await asyncio.to_thread(
                wizard.notes, body.get('status'))
            return web.json_response({'result': {'markdown': text}}, headers={'Cache-Control': 'no-store'})
        except bridge.BridgeError as exc:
            return web.json_response({'error': str(exc)}, status=502)
        except (ValueError, KeyError, TypeError) as exc:
            return web.json_response({'error': 'Invalid request: ' + str(exc)}, status=400)

    names = {'pair_code': 'pair-code', 'pair_cancel': 'pair-cancel', 'host_app_title': 'host-app-title',
             'access_decision': 'access-decision', 'request_access': 'request-access', 'session_router': 'session-router'}
    return [
        AppRoute('GET', '/rac/status', operation('status')),
        AppRoute('GET', '/rac/session-router', operation('session_router')),
        AppRoute('GET', '/rac/wizard-check', wizard_check),
        AppRoute('POST', '/rac/wizard-notes', wizard_notes),
        AppRoute('POST', '/rac/host-link', host_link),
    ] + [
        AppRoute('POST', '/rac/' + names.get(c, c), operation(c))
        for c in ('pair_code', 'pair_cancel', 'pair', 'send', 'set', 'unpair', 'ping', 'wifi', 'probe', 'speedtest', 'host_app_title',
                  'permissions', 'access_decision', 'request_access', 'devices', 'session_router')
    ]
