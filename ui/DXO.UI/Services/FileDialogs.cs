using System;
using System.Runtime.InteropServices;

namespace DXO.UI.Services;

/// The classic Windows "Open" and "Save as" dialogs.
///
/// DXO runs as administrator, and the WinRT file pickers refuse to open in an
/// elevated app. These comdlg32 dialogs work either way.
public static class FileDialogs
{
    /// Filter pairs: ("DXO profile", "*.toml"), ("All files", "*.*")...
    public static string? Open(string title, params (string Name, string Pattern)[] filters) =>
        Show(save: false, title, null, null, filters);

    /// `defaultExt` without the dot ("toml"); added if the user types a bare name.
    public static string? Save(string title, string suggestedName, string defaultExt, params (string Name, string Pattern)[] filters) =>
        Show(save: true, title, suggestedName, defaultExt, filters);

    private const int MaxPath = 2048;
    private const uint OFN_OVERWRITEPROMPT = 0x2, OFN_HIDEREADONLY = 0x4, OFN_NOCHANGEDIR = 0x8,
                       OFN_PATHMUSTEXIST = 0x800, OFN_FILEMUSTEXIST = 0x1000, OFN_EXPLORER = 0x80000;

    private static string? Show(bool save, string title, string? suggested, string? defaultExt, (string Name, string Pattern)[] filters)
    {
        // "Name\0pattern\0Name\0pattern\0\0": embedded NULs, so build it by hand.
        var sb = new System.Text.StringBuilder();
        foreach (var (name, pattern) in filters) sb.Append(name).Append('\0').Append(pattern).Append('\0');
        sb.Append('\0');

        IntPtr file = Marshal.AllocHGlobal(MaxPath * sizeof(char));
        IntPtr filter = Marshal.StringToHGlobalUni(sb.ToString());
        IntPtr titlePtr = Marshal.StringToHGlobalUni(title);
        IntPtr ext = defaultExt is null ? IntPtr.Zero : Marshal.StringToHGlobalUni(defaultExt);
        try
        {
            // Start with the suggested name (or an empty string) in the file buffer.
            string start = suggested ?? "";
            if (start.Length >= MaxPath) start = start[..(MaxPath - 1)];
            Marshal.Copy(start.ToCharArray(), 0, file, start.Length);
            Marshal.WriteInt16(file, start.Length * sizeof(char), 0);

            var ofn = new OPENFILENAMEW
            {
                lStructSize = (uint)Marshal.SizeOf<OPENFILENAMEW>(),
                hwndOwner = Owner(),
                lpstrFilter = filter,
                nFilterIndex = 1,
                lpstrFile = file,
                nMaxFile = MaxPath,
                lpstrTitle = titlePtr,
                lpstrDefExt = ext,
                Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_HIDEREADONLY | OFN_PATHMUSTEXIST |
                        (save ? OFN_OVERWRITEPROMPT : OFN_FILEMUSTEXIST),
            };
            bool ok = save ? GetSaveFileNameW(ref ofn) : GetOpenFileNameW(ref ofn);
            return ok ? Marshal.PtrToStringUni(file) : null;
        }
        catch (Exception ex)
        {
            App.LogCrash("FileDialogs", ex);
            return null;
        }
        finally
        {
            Marshal.FreeHGlobal(file);
            Marshal.FreeHGlobal(filter);
            Marshal.FreeHGlobal(titlePtr);
            if (ext != IntPtr.Zero) Marshal.FreeHGlobal(ext);
        }
    }

    private static IntPtr Owner()
    {
        try
        {
            return ((App)Microsoft.UI.Xaml.Application.Current).MainWindowRef is { } w
                ? WinRT.Interop.WindowNative.GetWindowHandle(w)
                : IntPtr.Zero;
        }
        catch { return IntPtr.Zero; }
    }

    // commdlg.h OPENFILENAMEW. Every string is a raw pointer we own.
    [StructLayout(LayoutKind.Sequential)]
    private struct OPENFILENAMEW
    {
        public uint lStructSize;
        public IntPtr hwndOwner;
        public IntPtr hInstance;
        public IntPtr lpstrFilter;
        public IntPtr lpstrCustomFilter;
        public uint nMaxCustFilter;
        public uint nFilterIndex;
        public IntPtr lpstrFile;
        public uint nMaxFile;
        public IntPtr lpstrFileTitle;
        public uint nMaxFileTitle;
        public IntPtr lpstrInitialDir;
        public IntPtr lpstrTitle;
        public uint Flags;
        public ushort nFileOffset;
        public ushort nFileExtension;
        public IntPtr lpstrDefExt;
        public IntPtr lCustData;
        public IntPtr lpfnHook;
        public IntPtr lpTemplateName;
        public IntPtr pvReserved;
        public uint dwReserved;
        public uint FlagsEx;
    }

    [DllImport("comdlg32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetOpenFileNameW(ref OPENFILENAMEW ofn);

    [DllImport("comdlg32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetSaveFileNameW(ref OPENFILENAMEW ofn);
}
