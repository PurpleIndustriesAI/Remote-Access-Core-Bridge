"""Talk to the local Remote Access Core (Bridge) and keep it running.

The bridge is a separate native process so clipboard sync keeps working when Crew is closed.
This module only proxies an allow-listed set of commands to its loopback control port."""
import json
import os
import socket
import subprocess
import sys
from pathlib import Path

APP_DIR = Path(__file__).resolve().parent.parent
DEFAULT_PORT = 48765

# Command -> fields the dashboard may pass through.
ALLOWED = {
    'status': set(),
    'pair_code': set(),
    'pair_cancel': set(),
    'pair': {'addr', 'code'},
    'send': {'peer', 'text', 'image_png_b64', 'paths'},
    'set': {'autosync', 'peer', 'receive_dir', 'moonlight_link', 'channels', 'allow_access_requests',
            'transport_wifi', 'transport_usb', 'transport_bluetooth'},
    'permissions': {'peer', 'capability', 'allowed'},
    'access_decision': {'peer', 'approve'},
    'request_access': {'peer', 'capabilities'},
    'host_app_title': set(),
    'unpair': {'peer'},
    'ping': {'peer'},
    'wifi': {'scan'},
    'probe': {'peer', 'n'},
    'speedtest': {'peer', 'mib'},
}


class BridgeError(Exception):
    pass


def home() -> Path:
    if os.environ.get('RAC_HOME'):
        return Path(os.environ['RAC_HOME'])
    if os.name == 'nt':
        return Path(os.environ['APPDATA']) / 'RemoteAccessCrew'
    base = os.environ.get('XDG_CONFIG_HOME') or str(Path.home() / '.config')
    return Path(base) / 'remote-access-crew'


def _port() -> int:
    try:
        st = json.loads((home() / 'state.json').read_text(encoding='utf-8'))
        return int(st['settings']['port'])
    except (OSError, ValueError, KeyError, TypeError):
        return DEFAULT_PORT


def binary() -> Path:
    name = 'remote-access-crew.exe' if os.name == 'nt' else 'remote-access-crew'
    return APP_DIR / 'bin' / name


def call(cmd: str, body: dict | None = None, timeout: float = 60.0):
    if cmd not in ALLOWED:
        raise ValueError('unknown command')
    body = body or {}
    extra = set(body) - ALLOWED[cmd]
    if extra:
        raise ValueError('unexpected fields: ' + ', '.join(sorted(extra)))
    try:
        token = (home() / 'control.token').read_text(encoding='utf-8').strip()
    except OSError as exc:
        raise BridgeError('The bridge has not started on this machine yet.') from exc
    req = dict(body, cmd=cmd, token=token)
    try:
        with socket.create_connection(('127.0.0.1', _port() + 1), timeout=timeout) as s:
            s.sendall((json.dumps(req) + '\n').encode())
            line = s.makefile('rb').readline(64 * 1024 * 1024)
    except OSError as exc:
        raise BridgeError('The Remote Access Core (Bridge) is not running.') from exc
    reply = json.loads(line or b'{}')
    if not reply.get('ok'):
        raise BridgeError(reply.get('error') or 'request failed')
    return reply.get('result')


def running() -> bool:
    try:
        call('status', timeout=2)
        return True
    except (BridgeError, ValueError):
        return False


def ensure_running(log_dir: Path) -> str:
    """Start the bridge hidden if it is not already running. Never steals focus."""
    if running():
        return 'running'
    exe = binary()
    if not exe.exists():
        return 'missing'
    log_dir.mkdir(parents=True, exist_ok=True)
    if os.name == 'nt':
        flags = 0x08000000 | 0x00000008 | 0x00000200  # CREATE_NO_WINDOW | DETACHED_PROCESS | NEW_PROCESS_GROUP
        subprocess.Popen([str(exe)], creationflags=flags, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, close_fds=True, cwd=str(APP_DIR))
    else:
        with open(log_dir / 'bridge.log', 'ab') as log:
            subprocess.Popen([str(exe)], stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                             start_new_session=True, close_fds=True, cwd=str(APP_DIR))
    return 'started'


SUNSHINE_STATE = Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Sunshine' / 'config' / 'sunshine_state.json'


def host_link_script() -> Path | None:
    for p in (APP_DIR / 'bin' / 'host-moonlight-link.ps1',
              Path(os.environ.get('LOCALAPPDATA', '')) / 'Programs' / 'RemoteAccessCrew' / 'host-moonlight-link.ps1'):
        if p.is_file():
            return p
    return None


def host_link_setup(remove: bool = False) -> str:
    """Run the host setup script hidden. Windows shows its own approval prompt once,
    because publishing the identity edits Sunshine's app list and restarts Sunshine."""
    if os.name != 'nt':
        raise BridgeError('Publishing to Sunshine is a Windows host step.')
    script = host_link_script()
    if script is None:
        raise BridgeError('host-moonlight-link.ps1 is missing; reinstall Remote Access Core (Bridge).')
    args = ['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(script)]
    if remove:
        args.append('-Remove')
    done = subprocess.run(args, capture_output=True, text=True, timeout=300, creationflags=0x08000000)
    out = (done.stdout or '').strip().splitlines()
    if done.returncode != 0:
        raise BridgeError(((done.stderr or '').strip().splitlines() or ['Sunshine update failed'])[-1])
    return out[-1] if out else 'Done.'


if __name__ == '__main__':
    print(json.dumps(call(sys.argv[1] if len(sys.argv) > 1 else 'status'), indent=2))
