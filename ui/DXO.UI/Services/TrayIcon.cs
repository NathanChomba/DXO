using System;
using System.Runtime.InteropServices;

namespace DXO.UI.Services;

/// Notification-area icon for the main window, plain Win32 (no extra package).
/// Hooks the window's message procedure to receive tray clicks, and also
/// handles:
///  * "TaskbarCreated" - Explorer restarted, so put the icon back;
///  * "DXO.ShowWindow" - a second copy of DXO was launched and asks this one to show itself.
public sealed class TrayIcon : IDisposable
{
    private const uint WM_APP_TRAY = 0x8000 + 1; // WM_APP + 1
    private const uint WM_LBUTTONUP = 0x0202, WM_RBUTTONUP = 0x0205, WM_LBUTTONDBLCLK = 0x0203, WM_CONTEXTMENU = 0x007B;
    private const uint NIM_ADD = 0, NIM_MODIFY = 1, NIM_DELETE = 2;
    private const uint NIF_MESSAGE = 1, NIF_ICON = 2, NIF_TIP = 4, NIF_INFO = 0x10;
    private const uint MF_STRING = 0, MF_SEPARATOR = 0x800, MF_GRAYED = 1;
    private const uint TPM_RETURNCMD = 0x100, TPM_RIGHTBUTTON = 2, TPM_BOTTOMALIGN = 0x20;
    private const int GWLP_WNDPROC = -4;
    private const int CMD_OPEN = 1, CMD_EXIT = 2;

    public static readonly uint ShowWindowMessage = RegisterWindowMessageW("DXO.ShowWindow");
    private static readonly uint TaskbarCreated = RegisterWindowMessageW("TaskbarCreated");

    public event Action? OpenRequested;
    public event Action? ExitRequested;

    private readonly IntPtr _hwnd;
    private readonly IntPtr _icon;
    private readonly WndProc _proc; // keep the delegate alive while Windows holds a pointer to it
    private readonly IntPtr _oldProc;
    private string _tip;
    private bool _added;

    public TrayIcon(IntPtr hwnd, string iconPath, string tip)
    {
        _hwnd = hwnd;
        _tip = tip;
        int cx = GetSystemMetrics(49 /* SM_CXSMICON */), cy = GetSystemMetrics(50 /* SM_CYSMICON */);
        _icon = LoadImageW(IntPtr.Zero, iconPath, 1 /* IMAGE_ICON */, cx, cy, 0x10 /* LR_LOADFROMFILE */);
        if (_icon == IntPtr.Zero) _icon = LoadIconW(IntPtr.Zero, (IntPtr)32512 /* IDI_APPLICATION */);

        _proc = Hook;
        _oldProc = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, Marshal.GetFunctionPointerForDelegate(_proc));

        // An elevated DXO must still hear from a normal one (UIPI blocks it otherwise).
        ChangeWindowMessageFilterEx(hwnd, ShowWindowMessage, 1 /* MSGFLT_ALLOW */, IntPtr.Zero);
        ChangeWindowMessageFilterEx(hwnd, TaskbarCreated, 1, IntPtr.Zero);
        Add();
    }

    public void SetTip(string tip)
    {
        _tip = tip;
        var d = Data(NIF_TIP);
        Shell_NotifyIconW(NIM_MODIFY, ref d);
    }

    public void ShowBalloon(string title, string text)
    {
        var d = Data(NIF_INFO);
        d.szInfoTitle = title;
        d.szInfo = text;
        d.dwInfoFlags = 0x1; // NIIF_INFO
        Shell_NotifyIconW(NIM_MODIFY, ref d);
    }

    private void Add()
    {
        var d = Data(NIF_MESSAGE | NIF_ICON | NIF_TIP);
        _added = Shell_NotifyIconW(NIM_ADD, ref d);
    }

    private NOTIFYICONDATAW Data(uint flags) => new()
    {
        cbSize = (uint)Marshal.SizeOf<NOTIFYICONDATAW>(),
        hWnd = _hwnd,
        uID = 1,
        uFlags = flags,
        uCallbackMessage = WM_APP_TRAY,
        hIcon = _icon,
        szTip = _tip,
        szInfo = "",
        szInfoTitle = "",
    };

    private IntPtr Hook(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        try
        {
            if (msg == WM_APP_TRAY)
            {
                uint ev = (uint)(lParam.ToInt64() & 0xFFFF);
                if (ev is WM_LBUTTONUP or WM_LBUTTONDBLCLK) OpenRequested?.Invoke();
                else if (ev is WM_RBUTTONUP or WM_CONTEXTMENU) ShowMenu();
                return IntPtr.Zero;
            }
            if (msg == ShowWindowMessage && msg != 0) { OpenRequested?.Invoke(); return IntPtr.Zero; }
            if (msg == TaskbarCreated && msg != 0) Add();
        }
        catch (Exception ex) { App.LogCrash("TrayIcon", ex); }
        return CallWindowProcW(_oldProc, hwnd, msg, wParam, lParam);
    }

    private void ShowMenu()
    {
        IntPtr menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, _tip);
        AppendMenuW(menu, MF_SEPARATOR, 0, null);
        AppendMenuW(menu, MF_STRING, CMD_OPEN, "Open DXO");
        AppendMenuW(menu, MF_STRING, CMD_EXIT, "Exit DXO");
        GetCursorPos(out var pt);
        SetForegroundWindow(_hwnd); // otherwise the menu won't close when you click elsewhere
        int cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.X, pt.Y, 0, _hwnd, IntPtr.Zero);
        DestroyMenu(menu);
        if (cmd == CMD_OPEN) OpenRequested?.Invoke();
        else if (cmd == CMD_EXIT) ExitRequested?.Invoke();
    }

    public void Dispose()
    {
        if (_added)
        {
            var d = Data(0);
            Shell_NotifyIconW(NIM_DELETE, ref d);
            _added = false;
        }
    }

    // ---- Win32 ----

    private delegate IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NOTIFYICONDATAW
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public uint uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT { public int X, Y; }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] private static extern bool Shell_NotifyIconW(uint msg, ref NOTIFYICONDATAW data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern uint RegisterWindowMessageW(string name);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr LoadImageW(IntPtr hinst, string name, uint type, int cx, int cy, uint flags);
    [DllImport("user32.dll")] private static extern IntPtr LoadIconW(IntPtr hinst, IntPtr name);
    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] private static extern IntPtr SetWindowLongPtrW(IntPtr hwnd, int index, IntPtr value);
    [DllImport("user32.dll")] private static extern IntPtr CallWindowProcW(IntPtr prev, IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] private static extern bool ChangeWindowMessageFilterEx(IntPtr hwnd, uint msg, uint action, IntPtr info);
    [DllImport("user32.dll")] private static extern IntPtr CreatePopupMenu();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern bool AppendMenuW(IntPtr menu, uint flags, nuint id, string? text);
    [DllImport("user32.dll")] private static extern int TrackPopupMenu(IntPtr menu, uint flags, int x, int y, int reserved, IntPtr hwnd, IntPtr rect);
    [DllImport("user32.dll")] private static extern bool DestroyMenu(IntPtr menu);
    [DllImport("user32.dll")] private static extern bool GetCursorPos(out POINT pt);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")] private static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    /// Ask an already-running DXO to show its window.
    public static void SignalOtherInstance() => PostMessageW((IntPtr)0xFFFF /* HWND_BROADCAST */, ShowWindowMessage, IntPtr.Zero, IntPtr.Zero);
}
