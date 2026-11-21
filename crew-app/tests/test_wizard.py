"""Unit tests for the setup wizard's read-only host checks and handoff notes.

These run anywhere with a plain Python 3: backend.wizard is stdlib-only,
so no Codex Crew installation is needed. Nothing here touches a real
bridge, service, or firewall: every subprocess call is faked.
"""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from backend import wizard  # noqa: E402

LINUX = sys.platform.startswith('linux')


def write_apps(path, apps):
    path.write_text(json.dumps({'env': {}, 'apps': apps}), encoding='utf-8')


GOOD_APPS = [
    {'name': 'Desktop', 'image-path': 'desktop.png'},
    {'name': 'Low Res Desktop', 'prep-cmd': [{'do': 'xrandr --output HDMI-1 --mode 1920x1080',
                                              'undo': 'xrandr --output HDMI-1 --mode 1920x1200'}]},
]


class AppsJsonTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'apps.json'

    def tearDown(self):
        self.tmp.cleanup()

    def test_distinct_desktop_entries_pass(self):
        write_apps(self.path, GOOD_APPS)
        result = wizard.check_apps_json(self.path)
        self.assertEqual(result['status'], 'ok')
        self.assertEqual(result['id'], 'apps_json')

    def test_duplicate_box_art_fails_with_fix(self):
        write_apps(self.path, GOOD_APPS + [{'name': 'Low Res Desktop', 'image-path': 'desktop.png'}])
        # The duplicate entry shadows the clean one in the name map.
        result = wizard.check_apps_json(self.path)
        self.assertEqual(result['status'], 'fail')
        self.assertIn('image-path', json.dumps(result))

    def test_missing_low_res_warns(self):
        write_apps(self.path, [{'name': 'Desktop', 'image-path': 'desktop.png'}])
        result = wizard.check_apps_json(self.path)
        self.assertEqual(result['status'], 'warn')
        self.assertIn('Low Res Desktop', result['detail'])

    def test_unreadable_file_fails_cleanly(self):
        result = wizard.check_apps_json(Path(self.tmp.name) / 'nope.json')
        self.assertEqual(result['status'], 'fail')
        self.assertNotIn('Traceback', result['detail'])

    def test_invalid_json_fails_cleanly(self):
        self.path.write_text('{not json', encoding='utf-8')
        result = wizard.check_apps_json(self.path)
        self.assertEqual(result['status'], 'fail')

    def test_custom_config_dir_env(self):
        with tempfile.TemporaryDirectory() as cfg:
            os.environ['RAC_SUNSHINE_CONFIG'] = cfg
            try:
                self.assertEqual(wizard.sunshine_config_dir(), Path(cfg))
            finally:
                del os.environ['RAC_SUNSHINE_CONFIG']


@unittest.skipUnless(LINUX, 'Linux-only permission check')
class UinputTests(unittest.TestCase):
    def test_correct_permissions_pass(self):
        with patch.object(os, 'stat') as fake:
            fake.return_value = type('st', (), {'st_mode': 0o100660, 'st_gid': 25})()
            result = wizard.check_uinput()
        self.assertEqual(result['status'], 'ok')

    def test_wrong_mode_fails_with_fix(self):
        with patch.object(os, 'stat') as fake:
            fake.return_value = type('st', (), {'st_mode': 0o100666, 'st_gid': 0})()
            result = wizard.check_uinput()
        self.assertEqual(result['status'], 'fail')
        self.assertIn('chmod', result['fix'])

    def test_missing_device_fails_with_fix(self):
        with patch.object(os, 'stat', side_effect=FileNotFoundError(2, 'No such file', '', '')):
            result = wizard.check_uinput()
        self.assertEqual(result['status'], 'fail')
        self.assertIn('modprobe', result['fix'])


class ServiceAndFirewallTests(unittest.TestCase):
    def test_linux_service_active_ok(self):
        with patch.object(wizard, '_run') as run:
            run.side_effect = [(0, 'active'), (0, 'enabled')]
            result = wizard.check_host_service()
        self.assertEqual(result['status'], 'ok')
        self.assertIn('active', result['detail'])

    def test_linux_service_inactive_fails(self):
        with patch.object(wizard, '_run') as run:
            run.side_effect = [(3, 'inactive'), (1, 'disabled')]
            result = wizard.check_host_service()
        self.assertEqual(result['status'], 'fail')
        self.assertIn('enable --now', result['fix'])

    def test_firewall_active_ok(self):
        with patch.object(wizard, '_run', return_value=(0, 'active')):
            result = wizard.check_firewall()
        self.assertEqual(result['status'], 'ok')

    def test_firewall_inactive_fails(self):
        with patch.object(wizard, '_run', return_value=(3, 'inactive')):
            result = wizard.check_firewall()
        self.assertEqual(result['status'], 'fail')

    def test_run_reports_error_type(self):
        rc, err = wizard._run(['definitely-not-a-real-binary-xyz'])
        self.assertIsNone(rc)
        self.assertEqual(err, 'FileNotFoundError')

    def test_detail_is_bounded(self):
        long = wizard._clip('x' * 600)
        self.assertLessEqual(len(long), wizard.DETAIL_MAX)
        self.assertTrue(long.endswith('…'))


class VirtualMicTests(unittest.TestCase):
    def test_present_on_linux_ok(self):
        out = '56\talsa_input.pci-0000\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n1306\trac_client_mic\tPipeWire\tfloat32le 1ch 48000Hz\tSUSPENDED'
        with patch.object(wizard, 'IS_LINUX', True), patch.object(wizard, '_run', return_value=(0, out)):
            result = wizard.check_virtual_mic()
        self.assertEqual(result['status'], 'ok')

    def test_monitor_or_feed_alone_is_not_the_mic(self):
        out = '1307\trac_client_mic_feed.monitor\tPipeWire\tfloat32le 1ch 48000Hz\tSUSPENDED'
        with patch.object(wizard, 'IS_LINUX', True), patch.object(wizard, '_run', return_value=(0, out)):
            result = wizard.check_virtual_mic()
        self.assertEqual(result['status'], 'fail')
        self.assertIn('rac-client-mic', result['fix'])

    def test_pactl_unavailable_warns(self):
        with patch.object(wizard, 'IS_LINUX', True), patch.object(wizard, '_run', return_value=(None, 'FileNotFoundError')):
            result = wizard.check_virtual_mic()
        self.assertEqual(result['status'], 'warn')


class NotesTests(unittest.TestCase):
    STATUS = {
        'name': 'Violet', 'short_fingerprint': '7EBE-A112', 'version': '0.7.0',
        'addrs': ['192.168.0.167:48765'],
        'peers': [{'name': 'MIKU', 'short_fingerprint': '2620-04AF'}],
        'settings': {'channels': ['crew-link', 'crew-sessions']},
    }

    def test_notes_compose_from_status_and_checks(self):
        with patch.object(wizard, 'inventory', return_value={'host': 'Violet', 'os': 'Omarchy', 'kernel': '7.2',
                                                              'gpu': 'GTX 1650', 'network': ['AX200; driver iwlwifi'],
                                                              'display_server': 'wayland'}), \
             patch.object(wizard, 'checks', return_value={'platform': 'linux', 'checks': [
                 {'id': 'host_service', 'label': 'RAC Host service', 'status': 'ok', 'detail': 'active', 'fix': ''},
                 {'id': 'uinput', 'label': '/dev/uinput permissions', 'status': 'ok', 'detail': '0660', 'fix': ''},
             ]}):
            text = wizard.notes(self.STATUS)
        self.assertIn('# RAC Setup Wizard Handoff Notes', text)
        self.assertIn('Violet', text)
        self.assertIn('**PASS** RAC Host service', text)
        self.assertIn('MIKU', text)
        self.assertIn('crew-link', text)
        self.assertIn('No secrets', text)

    def test_notes_fail_states_marked(self):
        with patch.object(wizard, 'inventory', return_value={'host': 'Violet'}), \
             patch.object(wizard, 'checks', return_value={'platform': 'linux', 'checks': [
                 {'id': 'firewall', 'label': 'LAN-only firewall', 'status': 'fail', 'detail': 'UFW inactive', 'fix': 'sudo ufw enable'},
             ]}):
            text = wizard.notes(self.STATUS)
        self.assertIn('**FAIL** LAN-only firewall', text)
        self.assertIn('UFW inactive', text)

    def test_checks_bundle_contains_all_five(self):
        with patch.object(wizard, 'check_host_service', return_value={'id': 'a'}), \
             patch.object(wizard, 'check_uinput', return_value={'id': 'b'}), \
             patch.object(wizard, 'check_apps_json', return_value={'id': 'c'}), \
             patch.object(wizard, 'check_firewall', return_value={'id': 'd'}), \
             patch.object(wizard, 'check_virtual_mic', return_value={'id': 'e'}):
            bundle = wizard.checks()
        self.assertEqual([c['id'] for c in bundle['checks']], ['a', 'b', 'c', 'd', 'e'])
        self.assertIn('platform', bundle)


LSPCI_SAMPLE = """\
06:00.0 Ethernet controller [0200]: Intel Corporation I210 [8086:1533] (rev 03)
\tSubsystem: ASUSTeK Computer Inc. Device [1043:8557]
\tKernel driver in use: igb
07:00.0 Network controller [0280]: Intel Corporation Wi-Fi 6 AX200 [8086:2723] (rev 1a)
\tSubsystem: Intel Corporation Wi-Fi 6 AX200NGW [8086:0084]
\tKernel driver in use: iwlwifi
00:1f.3 Audio device [0403]: Intel Corporation Device [8086:a348]
\tKernel driver in use: snd_hda_intel
"""


class NetworkParseTests(unittest.TestCase):
    def test_controllers_and_drivers_only(self):
        devices = wizard._network_devices(LSPCI_SAMPLE)
        self.assertEqual(devices, [
            '06:00.0 Ethernet controller [0200]: Intel Corporation I210 [8086:1533] (rev 03); driver igb',
            '07:00.0 Network controller [0280]: Intel Corporation Wi-Fi 6 AX200 [8086:2723] (rev 1a); driver iwlwifi',
        ])

    def test_blank_lines_between_blocks(self):
        devices = wizard._network_devices(LSPCI_SAMPLE.replace('07:00.0', '\n07:00.0'))
        self.assertEqual(len(devices), 2)
        self.assertIn('iwlwifi', devices[1])

    def test_no_controllers_is_empty(self):
        self.assertEqual(wizard._network_devices('00:00.0 Host bridge\n\tKernel driver in use: core\n'), [])

    def test_driverless_controller_still_listed(self):
        devices = wizard._network_devices('09:00.0 Network controller [0280]: Fake [1234:5678]\n')
        self.assertEqual(devices, ['09:00.0 Network controller [0280]: Fake [1234:5678]'])


if __name__ == '__main__':
    unittest.main()
