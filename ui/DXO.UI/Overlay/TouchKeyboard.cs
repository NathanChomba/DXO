using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;

namespace DXO.UI.Overlay;

/// Opens and closes Windows' own touch keyboard (the Settings opt-in).
///
/// Windows has no documented call for this. The widely used route is the
/// keyboard's own COM object, which toggles it, with TabTip.exe started
/// first if the keyboard service isn't running yet. Whether the keyboard
/// then appears in its gamepad layout is up to Windows: it does so when it
/// sees a gamepad in use, and DXO's virtual Xbox pad is one.
public static class TouchKeyboard
{
    [ComImport, Guid("4ce576fa-83dc-4F88-951c-9d0782b4e376")]
    private class UIHostNoLaunch { }

    [ComImport, Guid("37c994e7-432b-4834-a2f7-dce1f13b834b"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITipInvocation
    {
        void Toggle(IntPtr hwnd);
    }

    [DllImport("user32.dll")] private static extern IntPtr GetDesktopWindow();

    /// Show the keyboard if it is hidden, hide it if it is showing.
    /// Returns false if Windows wouldn't do it.
    public static bool Toggle()
    {
        if (TryToggle()) return true;
        // The keyboard's host isn't running: start it, give it a moment, try once more.
        try
        {
            string tabTip = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.CommonProgramFiles), "microsoft shared", "ink", "TabTip.exe");
            if (!File.Exists(tabTip)) return false;
            Process.Start(new ProcessStartInfo(tabTip) { UseShellExecute = true });
            System.Threading.Thread.Sleep(400);
        }
        catch (Exception ex)
        {
            App.LogCrash("TouchKeyboard.Start", ex);
            return false;
        }
        return TryToggle();
    }

    private static bool TryToggle()
    {
        object? host = null;
        try
        {
            host = new UIHostNoLaunch();
            ((ITipInvocation)host).Toggle(GetDesktopWindow());
            return true;
        }
        catch (Exception)
        {
            return false;
        }
        finally
        {
            if (host is not null) Marshal.ReleaseComObject(host);
        }
    }
}
