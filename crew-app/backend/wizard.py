"""Read-only host verification for the Remote Access Core setup wizard.

Every function here is non-destructive: it reads status, parses config
files, and runs bounded read-only queries. Nothing is installed, started,
stopped, or rewritten. Results carry no secrets - only versions, names,
permission bits, and firewall state. The dashboard exposes this only to
an unrestricted owner session, exactly like the other host routes.
"""
import json
import os
import socket
import stat as statmod
import subprocess
import sys
import time
from pathlib import Path

IS_LINUX = sys.platform.startswith('linux')
IS_WINDOWS = os.name == 'nt'
TIMEOUT = 10.0
DETAIL_MAX = 240

RAC_HOST_UNIT = 'rac-host.service'
WINDOWS_HOST_SERVICE = 'SunshineService'


def _clip(text, limit=DETAIL_MAX):
    text = ' '.join(str(text).split())
    return text if len(text) <= limit else text[:limit - 1] + '…'


def _run(args, timeout=TIMEOUT):
    """Run one read-only command. Returns (returncode, stdout) or (None, error)."""
    try:
        done = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
        return done.returncode, (done.stdout or '').strip()
    except (OSError, subprocess.SubprocessError) as exc:
        return None, type(exc).__name__


def _result(check_id, label, status, detail, fix=''):
    return {'id': check_id, 'label': label, 'status': status, 'detail': _clip(detail), 'fix': _clip(fix)}


def sunshine_config_dir():
    if os.environ.get('RAC_SUNSHINE_CONFIG'):
        return Path(os.environ['RAC_SUNSHINE_CONFIG'])
    if IS_WINDOWS:
        return Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Sunshine' / 'config'
    base = os.environ.get('XDG_CONFIG_HOME') or str(Path.home() / '.config')
    return Path(base) / 'sunshine'


def check_host_service():
    """RAC Host service running and enabled."""
    if IS_LINUX:
        rc, active = _run(['systemctl', '--user', 'is-active', RAC_HOST_UNIT], timeout=5)
        if rc is None:
            return _result('host_service', 'RAC Host service', 'warn',
                           f'Could not query systemd: {active}', 'Run: systemctl --user status rac-host')
        if active == 'active':
            rc2, enabled = _run(['systemctl', '--user', 'is-enabled', RAC_HOST_UNIT], timeout=5)
            note = '' if (rc2 == 0 and enabled == 'enabled') else f' (enabled={enabled or "unknown"})'
            return _result('host_service', 'RAC Host service', 'ok', f'active (running){note}')
        return _result('host_service', 'RAC Host service', 'fail',
                       f'rac-host is {active or "not installed"}',
                       'Run: systemctl --user enable --now rac-host.service')
    if IS_WINDOWS:
        rc, out = _run(['sc.exe', 'query', WINDOWS_HOST_SERVICE], timeout=5)
        if rc is None:
            return _result('host_service', 'RAC Host service', 'warn',
                           f'Could not query the service: {out}', f'Run: sc query {WINDOWS_HOST_SERVICE}')
        if 'RUNNING' in out.upper():
            return _result('host_service', 'RAC Host service', 'ok', 'Remote Access Core (Host) service running')
        return _result('host_service', 'RAC Host service', 'fail',
                       f'{WINDOWS_HOST_SERVICE} is not running',
                       f'Start the Remote Access Core (Host) service: sc start {WINDOWS_HOST_SERVICE}')
    return _result('host_service', 'RAC Host service', 'na', 'Not checkable on this platform.')


def check_uinput():
    """/dev/uinput writable by the input group only (virtual gamepad input)."""
    if not IS_LINUX:
        return _result('uinput', '/dev/uinput permissions', 'na', 'Linux host check only.')
    try:
        info = os.stat('/dev/uinput')
    except OSError as exc:
        return _result('uinput', '/dev/uinput permissions', 'fail',
                       f'/dev/uinput missing: {exc.strerror or type(exc).__name__}',
                       'Load the uinput module: sudo modprobe uinput')
    mode = statmod.S_IMODE(info.st_mode)
    group_ok = info.st_gid != 0
    mode_ok = mode == 0o660
    detail = f'{oct(mode)} gid={info.st_gid}'
    if mode_ok and group_ok:
        return _result('uinput', '/dev/uinput permissions', 'ok', f'/dev/uinput is 0660, group input ({detail})')
    return _result('uinput', '/dev/uinput permissions', 'fail', f'/dev/uinput is {detail}',
                   'Run: sudo chown root:input /dev/uinput && sudo chmod 0660 /dev/uinput')


def check_apps_json(path=None):
    """Desktop and Low Res Desktop present and clearly distinct (no duplicate box art)."""
    apps_path = Path(path) if path else sunshine_config_dir() / 'apps.json'
    try:
        apps = json.loads(apps_path.read_text(encoding='utf-8'))['apps']
    except OSError as exc:
        return _result('apps_json', 'Host apps.json desktop entries', 'fail',
                       f'{apps_path.name} unreadable: {exc.strerror or type(exc).__name__}',
                       'Reinstall the host or restore ~/.config/sunshine/apps.json')
    except (ValueError, KeyError, TypeError):
        return _result('apps_json', 'Host apps.json desktop entries', 'fail',
                       f'{apps_path.name} is not valid JSON with an apps list', 'Reinstall the host apps.json')
    names = {a.get('name'): a for a in apps if isinstance(a, dict)}
    desktop, lowres = names.get('Desktop'), names.get('Low Res Desktop')
    if desktop is None or lowres is None:
        missing = 'Low Res Desktop' if lowres is None else 'Desktop'
        return _result('apps_json', 'Host apps.json desktop entries', 'warn',
                       f'{missing} entry not found; entries: {", ".join(sorted(names)) or "none"}',
                       'Restore the default Desktop and Low Res Desktop entries from the RAC installer')
    if str(lowres.get('image-path', '')).lower() == 'desktop.png':
        return _result('apps_json', 'Host apps.json desktop entries', 'fail',
                       'Low Res Desktop reuses the Desktop desktop.png box art (duplicate tiles)',
                       'Remove the image-path from Low Res Desktop so its label stays distinct')
    return _result('apps_json', 'Host apps.json desktop entries', 'ok',
                   'Desktop and Low Res Desktop are present and clearly distinct')


def check_firewall():
    """Host firewall active; RAC ports must stay LAN/Tailscale scoped."""
    if IS_LINUX:
        rc, active = _run(['systemctl', 'is-active', 'ufw'], timeout=5)
        if rc is None:
            return _result('firewall', 'LAN-only firewall', 'warn',
                           f'Could not query the firewall: {active}', 'Run: sudo ufw status')
        if active != 'active':
            return _result('firewall', 'LAN-only firewall', 'fail', 'UFW is inactive',
                           'Run: sudo ufw enable, then allow RAC ports from RFC1918 ranges only')
        return _result('firewall', 'LAN-only firewall', 'ok',
                       'UFW active. RAC ports are restricted to RFC1918 LAN subnets and tailscale0.',
                       'Verify rules as root: sudo ufw status numbered')
    if IS_WINDOWS:
        rc, out = _run(['netsh', 'advfirewall', 'show', 'allprofiles', 'state'], timeout=5)
        if rc is None:
            return _result('firewall', 'LAN-only firewall', 'warn',
                           f'Could not query the firewall: {out}', 'Run: netsh advfirewall show allprofiles state')
        profiles_on = [line.split()[-1] for line in out.splitlines() if 'ON' in line.upper()]
        if len(profiles_on) >= 1:
            return _result('firewall', 'LAN-only firewall', 'ok', f'Windows Firewall on for {len(profiles_on)} profile(s)')
        return _result('firewall', 'LAN-only firewall', 'fail', 'Windows Firewall is off on all profiles',
                       'Run: netsh advfirewall set allprofiles state on')
    return _result('firewall', 'LAN-only firewall', 'na', 'Not checkable on this platform.')


def check_virtual_mic():
    """The host's virtual microphone a client's mic plays into (RAC Bridge 0.7.1+)."""
    if IS_LINUX:
        rc, out = _run(['pactl', 'list', 'short', 'sources'], timeout=5)
        if rc is None:
            return _result('microphone', 'Client microphone', 'warn',
                           f'Could not list audio sources: {out}', 'Install pipewire-pulse (pactl)')
        if any(line.split('\t')[1:2] == ['rac_client_mic'] for line in out.splitlines()):
            return _result('microphone', 'Client microphone', 'ok',
                           '"RAC Client Microphone" is available to host apps (voice chat, dictation, Command Core).')
        return _result('microphone', 'Client microphone', 'fail',
                       'The "RAC Client Microphone" virtual input is missing',
                       'Run: systemctl --user enable --now rac-client-mic')
    if IS_WINDOWS:
        return _result('microphone', 'Client microphone', 'na',
                       'Windows hosts play the client mic into Steam Streaming Microphone; see the README.')
    return _result('microphone', 'Client microphone', 'na', 'Not checkable on this platform.')


def _network_devices(lspci_out):
    """Parse `lspci -nnk` output for network controllers and their drivers.

    Works with or without blank lines between device blocks; only the
    controller lines and their driver lines are kept (bounded, no serials).
    """
    devices, block, driver = [], None, ''
    for line in lspci_out.splitlines():
        if line and not line[0].isspace():
            if block is not None:
                devices.append('; '.join(block + ([f'driver {driver}'] if driver else [])))
            block, driver = None, ''
            if 'Network controller' in line or 'Ethernet controller' in line:
                block = [line.strip()]
        elif block is not None and 'Kernel driver in use' in line:
            driver = line.split(':', 1)[-1].strip()
    if block is not None:
        devices.append('; '.join(block + ([f'driver {driver}'] if driver else [])))
    return devices


def inventory():
    """Bounded machine facts for the handoff notes. Read-only, no secrets."""
    facts = {'platform': sys.platform}
    if IS_LINUX:
        rc, kernel = _run(['uname', '-r'], timeout=5)
        if rc == 0:
            facts['kernel'] = kernel
        try:
            with open('/etc/os-release', encoding='utf-8') as fh:
                for line in fh:
                    if line.startswith('PRETTY_NAME='):
                        facts['os'] = line.split('=', 1)[1].strip().strip('"')
                        break
        except OSError:
            pass
        rc, gpu = _run(['nvidia-smi', '--query-gpu=name,driver_version', '--format=csv,noheader'], timeout=8)
        if rc == 0 and gpu:
            facts['gpu'] = gpu.splitlines()[0]
        rc, wifi = _run(['lspci', '-nnk'], timeout=8)
        if rc == 0:
            devices = _network_devices(wifi)
            if devices:
                facts['network'] = devices
        if os.environ.get('XDG_SESSION_TYPE'):
            facts['display_server'] = os.environ['XDG_SESSION_TYPE']
    elif IS_WINDOWS:
        facts['os'] = f'Windows {sys.getwindowsversion().release}'  # type: ignore[attr-defined]
    try:
        facts['host'] = socket.gethostname()
    except OSError:
        pass
    return facts


def checks():
    """All host checks for the wizard, safe to run in a worker thread."""
    return {'platform': sys.platform, 'checks': [
        check_host_service(), check_uinput(), check_apps_json(), check_firewall(), check_virtual_mic()]}


def _notes_facts(status):
    facts = inventory()
    checks_result = checks()
    return facts, checks_result


def notes(status=None):
    """Compose the machine-side handoff notes (markdown) from live state.

    ``status`` is the bridge ``status`` result dict; when omitted it is
    fetched directly from the local bridge (owner route already gates this).
    """
    from . import bridge  # local import keeps this module import-light for tests

    if status is None:
        status = bridge.call('status', timeout=15)
    facts, checks_result = _notes_facts(status)
    peers = status.get('peers') or []
    settings = status.get('settings') or {}
    channels = settings.get('channels') or []
    lines = [
        '# RAC Setup Wizard Handoff Notes',
        '',
        f'Generated by the RAC GUI setup wizard on **{facts.get("host", "this machine")}** '
        f'({status.get("name", "?")} {status.get("short_fingerprint", "")}). {time.strftime("%Y-%m-%d %H:%M %Z")}.',
        '',
        '## Machine',
        f'- OS: {facts.get("os", "unknown")}, kernel {facts.get("kernel", "unknown")}',
        f'- GPU: {facts.get("gpu", "not reported")}',
        f'- Network hardware: ' + ('; '.join(facts.get('network', [])) or 'not reported'),
        f'- Display server: {facts.get("display_server", "unknown")}',
        f'- Bridge version: v{status.get("version", "?")}, reachable at {status.get("addrs", ["?"])[0]}',
        f'- Paired peers: ' + (', '.join(f"{p.get('name')} ({p.get('short_fingerprint')})" for p in peers) or 'none'),
        f'- Channels enabled: ' + (', '.join(channels) or 'none'),
        '',
        '## Host verification checks',
    ]
    for item in checks_result['checks']:
        mark = {'ok': 'PASS', 'warn': 'WARN', 'fail': 'FAIL'}.get(item['status'], 'N/A')
        lines.append(f'- **{mark}** {item["label"]}: {item["detail"]}')
    lines += [
        '',
        '## Security boundaries',
        '- No secrets, tokens, or pairing keys are included in these notes.',
        '- RAC ports stay restricted to RFC1918 LAN subnets and tailscale; verify with your firewall check above.',
        '- Every peer keeps explicit per-capability consent (clipboard, files, sessions).',
        '',
    ]
    return '\n'.join(lines)
