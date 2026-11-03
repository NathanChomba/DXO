// M4: P/Invoke bindings for dxo_core.dll (crates/dxo-core/src/ffi.rs).
// Keep every struct here byte-for-byte identical to its #[repr(C)] twin.
// ffi.rs has a test asserting the sizes (DxoState = 64, DxoOutput = 16).

using System;
using System.Runtime.InteropServices;
using System.Text;

namespace DXO.UI.Native;

[StructLayout(LayoutKind.Sequential)]
public struct DxoState
{
    public byte Connected;
    public byte Kind;           // 0 none, 1 DS4, 2 DualSense, 3 Edge, 4 generic
    public byte Transport;      // 0 USB, 1 BT
    public byte BatteryPercent; // 255 = unknown
    public byte BatteryStatus;  // 0 unknown, 1 discharging, 2 charging, 3 full
    private byte _p0, _p1, _p2;
    public uint Buttons;
    public uint ReportsPerSec;
    public float LX, LY, RX, RY, L2, R2;
    public short GyroX, GyroY, GyroZ;
    public short AccelX, AccelY, AccelZ;
    public byte Touch0Active, Touch1Active;
    private byte _p3, _p4;
    public ushort Touch0X, Touch0Y, Touch1X, Touch1Y;
}

[StructLayout(LayoutKind.Sequential)]
public struct DxoTrigger
{
    public byte Mode;     // 0 off, 1 feedback, 2 weapon
    public byte Start;
    public byte End;
    public byte Strength; // 1..8
}

[StructLayout(LayoutKind.Sequential)]
public struct DxoOutput
{
    public byte R, G, B;
    public byte PlayerLeds;
    public byte RumbleLeft, RumbleRight;
    private byte _p0, _p1;
    public DxoTrigger TriggerLeft;
    public DxoTrigger TriggerRight;
}

/// Virtual-device setup + live status (ffi.rs DxoVirtual, 8 bytes).
[StructLayout(LayoutKind.Sequential)]
public struct DxoVirtual
{
    public byte Target;            // 0 none (native), 1 Xbox 360, 2 DualShock 4
    public byte HidePhysical;
    public byte RumblePassthrough;
    public byte PadHidden;         // live: HidHide is hiding the real pad
    public byte VirtualActive;     // live: a virtual pad is plugged in
    private byte _p0, _p1, _p2;
}

/// Motion page readings + gyro feature status (ffi.rs DxoMotion, 32 bytes).
[StructLayout(LayoutKind.Sequential)]
public struct DxoMotion
{
    public float GyroPitch, GyroYaw, GyroRoll;   // calibrated deg/s
    public float AccelX, AccelY, AccelZ;         // g
    public byte CalState;      // 0 uncalibrated, 1 calibrating, 2 calibrated, 3 failed
    public byte CemuListening;
    public byte CemuFailed;    // switched on but the port couldn't be opened
    public byte CemuClients;
    public ushort CemuPort;
    public byte MouseActive;   // gyro is moving the mouse right now
    private byte _p0;
}

/// Diagnostics: report-arrival stats (ffi.rs DxoTiming, 32 bytes).
[StructLayout(LayoutKind.Sequential)]
public struct DxoTiming
{
    public uint Samples;
    public float AvgMs, MinMs, MaxMs, JitterMs;
    public uint Late;          // gaps over twice the average
    public uint ReportsTotal;
    public uint ReportLen;
}

/// One connected controller (ffi.rs DxoPadInfo, 144 bytes).
[StructLayout(LayoutKind.Sequential)]
public unsafe struct DxoPadInfo
{
    public byte Slot;            // 0..3 = player 1..4
    public byte Selected;        // per-controller calls act on this pad
    public byte Kind, Transport, BatteryPercent, BatteryStatus;
    public byte PadHidden, VirtualActive;
    public byte Target;          // 0 native, 1 Xbox 360, 2 DualShock 4
    private byte _p0, _p1, _p2;
    public uint ReportsPerSec;
    public fixed byte Name[64];  // UTF-8, NUL-terminated
    public fixed byte Profile[64];
}

/// DxoPadInfo with real strings, for the UI.
public sealed record PadInfo(int Slot, bool Selected, byte Kind, bool Bluetooth, byte BatteryPercent, byte BatteryStatus,
                             bool Hidden, bool VirtualActive, byte Target, uint ReportsPerSec, string Name, string Profile);

/// Bit positions of PadState.buttons (state.rs `Buttons`).
[Flags]
public enum DxoButtons : uint
{
    South = 1 << 0, East = 1 << 1, West = 1 << 2, North = 1 << 3,
    L1 = 1 << 4, R1 = 1 << 5, L3 = 1 << 6, R3 = 1 << 7,
    Select = 1 << 8, Start = 1 << 9, Home = 1 << 10, Touchpad = 1 << 11, Mute = 1 << 12,
    Up = 1 << 13, Down = 1 << 14, Left = 1 << 15, Right = 1 << 16,
    L2 = 1 << 17, R2 = 1 << 18, PaddleL = 1 << 19, PaddleR = 1 << 20,
}

internal static partial class DxoNative
{
    private const string Dll = "dxo_core.dll";
    public const uint ExpectedAbi = 7;

    [LibraryImport(Dll)] public static partial uint dxo_abi_version();

    [LibraryImport(Dll, StringMarshalling = StringMarshalling.Utf8)]
    public static partial IntPtr dxo_create(string? profilesDir, byte enableVirtual);

    [LibraryImport(Dll)] public static partial void dxo_destroy(IntPtr engine);
    [LibraryImport(Dll)] public static partial byte dxo_get_state(IntPtr engine, out DxoState state);
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_get_product_name(IntPtr engine, byte* buf, nuint len);
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_get_last_error(IntPtr engine, byte* buf, nuint len);
    [LibraryImport(Dll)] public static unsafe partial void dxo_set_output(IntPtr engine, DxoOutput* output);

    [LibraryImport(Dll, StringMarshalling = StringMarshalling.Utf8)]
    public static partial byte dxo_load_profile(IntPtr engine, string path);

    [LibraryImport(Dll)] public static partial byte dxo_get_virtual(IntPtr engine, out DxoVirtual v);
    [LibraryImport(Dll)] public static partial void dxo_set_virtual(IntPtr engine, byte target, byte hidePhysical, byte rumblePassthrough);
    [LibraryImport(Dll)] public static partial void dxo_reconnect(IntPtr engine);
    [LibraryImport(Dll)] public static partial void dxo_set_include_generic(IntPtr engine, byte on);

    // ---- multiple controllers (ABI 7) ----
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_list_pads(IntPtr engine, DxoPadInfo* pads, nuint max);
    [LibraryImport(Dll)] public static partial void dxo_select_pad(IntPtr engine, byte slot);
    [LibraryImport(Dll)] public static partial byte dxo_get_selected_pad(IntPtr engine);

    // ---- profile editor (ABI 3) ----
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_get_active_profile_json(IntPtr engine, byte* buf, nuint len);
    [LibraryImport(Dll, StringMarshalling = StringMarshalling.Utf8)]
    public static partial byte dxo_set_active_profile_json(IntPtr engine, string json);
    [LibraryImport(Dll, StringMarshalling = StringMarshalling.Utf8)]
    public static unsafe partial nuint dxo_profile_file_to_json(string path, byte* buf, nuint len);
    [LibraryImport(Dll, StringMarshalling = StringMarshalling.Utf8)]
    public static partial byte dxo_profile_json_to_file(string json, string path);
    [LibraryImport(Dll)] public static partial void dxo_reload_profiles(IntPtr engine);

    // ---- motion (ABI 4) ----
    [LibraryImport(Dll)] public static partial byte dxo_get_motion(IntPtr engine, out DxoMotion m);
    [LibraryImport(Dll)] public static partial void dxo_calibrate_gyro(IntPtr engine);

    // ---- diagnostics (ABI 5) ----
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_get_timing(IntPtr engine, out DxoTiming t, float* samples, nuint max);
    [LibraryImport(Dll)] public static unsafe partial nuint dxo_get_last_report(IntPtr engine, byte* buf, nuint len);
}

/// Safe, disposable wrapper. One instance for the app's lifetime.
public sealed unsafe class DxoEngine : IDisposable
{
    private IntPtr _h;

    public DxoEngine(string? profilesDir, bool enableVirtual = true)
    {
        uint abi = DxoNative.dxo_abi_version();
        if (abi != DxoNative.ExpectedAbi)
            throw new InvalidOperationException($"dxo_core.dll ABI {abi}, UI expects {DxoNative.ExpectedAbi}");
        _h = DxoNative.dxo_create(profilesDir, (byte)(enableVirtual ? 1 : 0));
        if (_h == IntPtr.Zero) throw new InvalidOperationException("dxo_create failed");
    }

    public DxoState GetState()
    {
        DxoNative.dxo_get_state(_h, out var s);
        return s;
    }

    public string ProductName => ReadString(DxoNative.dxo_get_product_name);
    public string LastError => ReadString(DxoNative.dxo_get_last_error);

    private unsafe delegate nuint StrFn(IntPtr h, byte* buf, nuint len);

    // Two-call pattern: ask for the size, then fill. Retries once if the
    // string grew in between (the engine thread may update it).
    private unsafe string ReadString(StrFn fn)
    {
        for (int attempt = 0; attempt < 2; attempt++)
        {
            nuint need = fn(_h, null, 0);
            if (need <= 1) return string.Empty;
            var buf = new byte[(int)need];
            nuint got;
            fixed (byte* p = buf) got = fn(_h, p, need);
            if (got <= need) return Encoding.UTF8.GetString(buf, 0, (int)got - 1);
        }
        return string.Empty;
    }

    public unsafe void SetOutput(DxoOutput? o)
    {
        if (o is DxoOutput v) DxoNative.dxo_set_output(_h, &v);
        else DxoNative.dxo_set_output(_h, null);
    }

    public bool LoadProfile(string path) => DxoNative.dxo_load_profile(_h, path) != 0;

    public DxoVirtual GetVirtual()
    {
        DxoNative.dxo_get_virtual(_h, out var v);
        return v;
    }

    /// target: 0 none (native), 1 Xbox 360, 2 DualShock 4
    public void SetVirtual(byte target, bool hidePhysical, bool rumblePassthrough) =>
        DxoNative.dxo_set_virtual(_h, target, (byte)(hidePhysical ? 1 : 0), (byte)(rumblePassthrough ? 1 : 0));

    /// Also use Switch Pro / generic gamepads (next connect).
    public void SetIncludeGeneric(bool on) => DxoNative.dxo_set_include_generic(_h, (byte)(on ? 1 : 0));

    public void Reconnect() => DxoNative.dxo_reconnect(_h);

    // ---- profiles ----
    public string ActiveProfileJson => ReadString(DxoNative.dxo_get_active_profile_json);
    public bool SetActiveProfileJson(string json) => DxoNative.dxo_set_active_profile_json(_h, json) != 0;
    public void ReloadProfiles() => DxoNative.dxo_reload_profiles(_h);

    /// TOML file -> JSON via the engine's own parser. Empty string if invalid.
    public static string ProfileFileToJson(string path)
    {
        for (int attempt = 0; attempt < 2; attempt++)
        {
            nuint need = DxoNative.dxo_profile_file_to_json(path, null, 0);
            if (need <= 1) return string.Empty;
            var buf = new byte[(int)need];
            nuint got;
            fixed (byte* p = buf) got = DxoNative.dxo_profile_file_to_json(path, p, need);
            if (got <= need) return Encoding.UTF8.GetString(buf, 0, (int)got - 1);
        }
        return string.Empty;
    }

    /// JSON -> validated TOML file via the engine. False if the JSON was rejected.
    public static bool ProfileJsonToFile(string json, string path) => DxoNative.dxo_profile_json_to_file(json, path) != 0;

    // ---- multiple controllers ----
    /// Connected controllers in slot order. Every other per-controller call
    /// acts on the selected one (SelectPad).
    public System.Collections.Generic.List<PadInfo> ListPads()
    {
        const int Max = 4;
        var buf = stackalloc DxoPadInfo[Max];
        int n = (int)Math.Min((nuint)Max, DxoNative.dxo_list_pads(_h, buf, Max));
        var list = new System.Collections.Generic.List<PadInfo>(n);
        for (int i = 0; i < n; i++)
        {
            DxoPadInfo* p = buf + i; // stack memory: fixed buffers are plain pointers here
            string name = Utf8Z(p->Name, 64), profile = Utf8Z(p->Profile, 64);
            list.Add(new PadInfo(p->Slot, p->Selected != 0, p->Kind, p->Transport == 1, p->BatteryPercent, p->BatteryStatus,
                                 p->PadHidden != 0, p->VirtualActive != 0, p->Target, p->ReportsPerSec, name, profile));
        }
        return list;
    }

    private static string Utf8Z(byte* p, int max)
    {
        int len = 0;
        while (len < max && p[len] != 0) len++;
        return Encoding.UTF8.GetString(p, len);
    }

    public void SelectPad(int slot) => DxoNative.dxo_select_pad(_h, (byte)slot);
    public int SelectedPad => DxoNative.dxo_get_selected_pad(_h);

    // ---- motion ----
    public DxoMotion GetMotion()
    {
        DxoNative.dxo_get_motion(_h, out var m);
        return m;
    }

    /// Measure gyro drift over the next 2 s; the controller should lie still.
    public void CalibrateGyro() => DxoNative.dxo_calibrate_gyro(_h);

    // ---- diagnostics ----
    /// Stats plus the most recent report gaps (ms, oldest first) written into `samples`.
    public DxoTiming GetTiming(float[] samples, out int count)
    {
        DxoTiming t;
        nuint n;
        fixed (float* p = samples) n = DxoNative.dxo_get_timing(_h, out t, p, (nuint)samples.Length);
        count = (int)n;
        return t;
    }

    public byte[] LastReport()
    {
        var buf = new byte[1024];
        nuint n;
        fixed (byte* p = buf) n = DxoNative.dxo_get_last_report(_h, p, (nuint)buf.Length);
        Array.Resize(ref buf, (int)Math.Min(n, (nuint)buf.Length));
        return buf;
    }

    public void Dispose()
    {
        if (_h != IntPtr.Zero)
        {
            DxoNative.dxo_destroy(_h); // stops the engine, unhides the pad
            _h = IntPtr.Zero;
        }
    }
}
