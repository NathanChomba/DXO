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
    public const uint ExpectedAbi = 1;

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
}

/// Safe, disposable wrapper. One instance for the app's lifetime.
public sealed class DxoEngine : IDisposable
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

    public void Dispose()
    {
        if (_h != IntPtr.Zero)
        {
            DxoNative.dxo_destroy(_h); // stops the engine, unhides the pad
            _h = IntPtr.Zero;
        }
    }
}
