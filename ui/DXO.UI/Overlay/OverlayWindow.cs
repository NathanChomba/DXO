using System;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace DXO.UI.Overlay;

/// Milestone 7 foundation: a window that floats over whatever app is in front
/// and never takes focus from it. The on-screen keyboard is the first user;
/// the radial quick-menu will be the second.
///
///  * always on top, no title bar, not in the taskbar or Alt+Tab
///  * WS_EX_NOACTIVATE: showing it or clicking it leaves the other app active,
///    so keys DXO types still land there
///  * shown and hidden with ShowWindow, never Activate()
///
/// It cannot draw over an exclusive-fullscreen game (nothing can without
/// injecting into the game, which DXO does not do). Borderless and windowed
/// games are fine.
public class OverlayWindow : Window
{
    protected IntPtr Hwnd { get; }

    public bool IsShown { get; private set; }

    public OverlayWindow()
    {
        Hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);

        var presenter = OverlappedPresenter.Create();
        presenter.IsResizable = false;
        presenter.IsMaximizable = false;
        presenter.IsMinimizable = false;
        presenter.IsAlwaysOnTop = true;
        presenter.SetBorderAndTitleBar(false, false);
        AppWindow.SetPresenter(presenter);
        try { AppWindow.IsShownInSwitchers = false; } catch { /* the tool-window style below covers it */ }

        long ex = GetWindowLongPtrW(Hwnd, GWL_EXSTYLE).ToInt64();
        SetWindowLongPtrW(Hwnd, GWL_EXSTYLE, new IntPtr(ex | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW));

        // Rounded corners on Windows 11; Windows 10 ignores this and stays square.
        int round = DWMWCP_ROUND;
        DwmSetWindowAttribute(Hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, ref round, sizeof(int));
    }

    /// Put the window on screen without making it the active window.
    public void ShowNoActivate()
    {
        ShowWindow(Hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(Hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        IsShown = true;
    }

    public void HideOverlay()
    {
        ShowWindow(Hwnd, SW_HIDE);
        IsShown = false;
    }

    /// Size the window and sit it at the bottom centre of the screen that the
    /// app in front is on. `widthFraction` is the share of that screen's width;
    /// `aspect` is height / width.
    protected void PlaceBottomCentre(double widthFraction, double aspect)
    {
        IntPtr fg = GetForegroundWindow();
        var id = Win32Interop.GetWindowIdFromWindow(fg != IntPtr.Zero ? fg : Hwnd);
        RectInt32 area = DisplayArea.GetFromWindowId(id, DisplayAreaFallback.Primary).WorkArea;

        const int Margin = 24;
        int maxW = Math.Max(320, area.Width - 2 * Margin);
        int maxH = Math.Max(160, (int)(area.Height * 0.6));
        int w = (int)Math.Clamp(area.Width * widthFraction, 320, maxW);
        int h = (int)(w * aspect);
        if (h > maxH)
        {
            h = maxH;
            w = (int)(h / aspect);
        }
        int x = area.X + (area.Width - w) / 2;
        int y = area.Y + area.Height - h - Margin;
        AppWindow.MoveAndResize(new RectInt32(x, y, w, h));
    }

    /// A square window in the middle of the screen that the app in front is
    /// on; `heightFraction` is its side as a share of that screen's height.
    protected void PlaceCentre(double heightFraction)
    {
        IntPtr fg = GetForegroundWindow();
        var id = Win32Interop.GetWindowIdFromWindow(fg != IntPtr.Zero ? fg : Hwnd);
        RectInt32 area = DisplayArea.GetFromWindowId(id, DisplayAreaFallback.Primary).WorkArea;
        int side = (int)Math.Clamp(area.Height * heightFraction, 240, Math.Min(area.Width, area.Height) - 48);
        AppWindow.MoveAndResize(new RectInt32(area.X + (area.Width - side) / 2, area.Y + (area.Height - side) / 2, side, side));
    }

    // ---- Win32 ----

    private const int GWL_EXSTYLE = -20;
    private const long WS_EX_TOOLWINDOW = 0x00000080;
    private const long WS_EX_NOACTIVATE = 0x08000000;
    private const int SW_HIDE = 0;
    private const int SW_SHOWNOACTIVATE = 4;
    private static readonly IntPtr HWND_TOPMOST = new(-1);
    private const uint SWP_NOSIZE = 0x0001, SWP_NOMOVE = 0x0002, SWP_NOACTIVATE = 0x0010;
    private const int DWMWA_WINDOW_CORNER_PREFERENCE = 33;
    private const int DWMWCP_ROUND = 2;

    [DllImport("user32.dll")] private static extern IntPtr GetWindowLongPtrW(IntPtr hwnd, int index);
    [DllImport("user32.dll")] private static extern IntPtr SetWindowLongPtrW(IntPtr hwnd, int index, IntPtr value);
    [DllImport("user32.dll")] private static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll")] private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);
}
