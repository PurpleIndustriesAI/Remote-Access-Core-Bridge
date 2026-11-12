using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

public class Program {
    private delegate IntPtr LowLevelKeyboardProc(int nCode, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern IntPtr SetWindowsHookEx(int idHook, LowLevelKeyboardProc lpfn, IntPtr hMod, uint dwThreadId);

    [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool UnhookWindowsHookEx(IntPtr hhk);

    [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);

    [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern IntPtr GetModuleHandle(string lpModuleName);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool GetMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax);

    [DllImport("user32.dll")]
    private static extern bool TranslateMessage([In] ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage([In] ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern void PostQuitMessage(int nExitCode);

    [StructLayout(LayoutKind.Sequential)]
    private struct MSG {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public int pt_x;
        public int pt_y;
    }

    private const int WH_KEYBOARD_LL = 13;
    private const int WM_KEYDOWN = 0x0100;
    private const int WM_SYSKEYDOWN = 0x0104;
    private const int LLKHF_INJECTED = 0x0010;

    private const uint WM_SYSCOMMAND = 0x0112;
    private const uint SC_MONITORPOWER = 0xF170;

    private static IntPtr _hookID = IntPtr.Zero;
    private static LowLevelKeyboardProc _proc = HookCallback;
    private static DateTime _lastPhysicalKeyPress = DateTime.UtcNow;
    private static bool _displayIsOff = false;

    private static void Log(string msg) {
        try {
            string p = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "SunshineDisplayWake");
            if (!Directory.Exists(p)) Directory.CreateDirectory(p);
            File.AppendAllText(Path.Combine(p, "display-wake.log"), DateTime.UtcNow.ToString("o") + " " + msg + Environment.NewLine);
        } catch {}
    }

    private static void SetDisplayPower(bool turnOn) {
        try {
            if (turnOn) {
                SendMessage((IntPtr)0xFFFF, WM_SYSCOMMAND, (IntPtr)SC_MONITORPOWER, (IntPtr)(-1));
                _displayIsOff = false;
                Log("DISPLAY_ON (woken by physical keyboard)");
            } else {
                SendMessage((IntPtr)0xFFFF, WM_SYSCOMMAND, (IntPtr)SC_MONITORPOWER, (IntPtr)2);
                _displayIsOff = true;
                Log("DISPLAY_OFF (blanked for remote stream)");
            }
        } catch (Exception ex) {
            Log("ERROR SetDisplayPower: " + ex.Message);
        }
    }

    private static IntPtr HookCallback(int nCode, IntPtr wParam, IntPtr lParam) {
        if (nCode >= 0 && (wParam == (IntPtr)WM_KEYDOWN || wParam == (IntPtr)WM_SYSKEYDOWN)) {
            int flags = Marshal.ReadInt32(lParam, 8);
            bool isInjected = (flags & LLKHF_INJECTED) != 0;
            if (!isInjected) {
                _lastPhysicalKeyPress = DateTime.UtcNow;
                if (_displayIsOff) {
                    SetDisplayPower(true);
                }
            }
        }
        return CallNextHookEx(_hookID, nCode, wParam, lParam);
    }

    private static bool IsSunshineStreaming() {
        try {
            string log = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), @"Sunshine\config\sunshine.log");
            if (!File.Exists(log)) return false;
            using (var fs = new FileStream(log, FileMode.Open, FileAccess.Read, FileShare.ReadWrite)) {
                if (fs.Length > 65536) fs.Seek(-65536, SeekOrigin.End);
                using (var reader = new StreamReader(fs)) {
                    string tail = reader.ReadToEnd();
                    int c = tail.LastIndexOf("CLIENT CONNECTED", StringComparison.OrdinalIgnoreCase);
                    int d = tail.LastIndexOf("CLIENT DISCONNECTED", StringComparison.OrdinalIgnoreCase);
                    return c > d;
                }
            }
        } catch { return false; }
    }

    public static int Main(string[] args) {
        if (args.Length > 0 && args[0] == "stop") {
            try {
                var ev = EventWaitHandle.OpenExisting("Global\\SunshineDisplayWake.Stop");
                ev.Set();
                ev.Close();
                Console.WriteLine("Stopped SunshineDisplayWake.");
                return 0;
            } catch {
                Console.WriteLine("SunshineDisplayWake not running.");
                return 0;
            }
        }

        bool createdNew;
        using (var mutex = new Mutex(true, "Global\\SunshineDisplayWake.Mutex", out createdNew)) {
            if (!createdNew) {
                Console.WriteLine("Already running.");
                return 0;
            }

            Log("STARTED");
            var stopEvent = new EventWaitHandle(false, EventResetMode.ManualReset, "Global\\SunshineDisplayWake.Stop");

            using (var curProcess = Process.GetCurrentProcess())
            using (var curModule = curProcess.MainModule) {
                _hookID = SetWindowsHookEx(WH_KEYBOARD_LL, _proc, GetModuleHandle(curModule.ModuleName), 0);
            }

            if (_hookID == IntPtr.Zero) {
                Log("HOOK_FAILED err=" + Marshal.GetLastWin32Error());
                return 1;
            }

            var monitorThread = new Thread(() => {
                while (!stopEvent.WaitOne(1000)) {
                    bool streaming = IsSunshineStreaming();
                    if (streaming) {
                        double idleSecs = (DateTime.UtcNow - _lastPhysicalKeyPress).TotalSeconds;
                        if (idleSecs > 90 && !_displayIsOff) {
                            SetDisplayPower(false);
                        }
                    } else {
                        if (_displayIsOff) {
                            SetDisplayPower(true);
                        }
                    }
                }
                PostQuitMessage(0);
            });
            monitorThread.IsBackground = true;
            monitorThread.Start();

            MSG msg;
            while (GetMessage(out msg, IntPtr.Zero, 0, 0)) {
                TranslateMessage(ref msg);
                DispatchMessage(ref msg);
            }

            UnhookWindowsHookEx(_hookID);
            SetDisplayPower(true);
            Log("STOPPED");
            return 0;
        }
    }
}
