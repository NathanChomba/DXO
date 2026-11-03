using System;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Dispatching;
using DXO.UI.Native;

namespace DXO.UI.ViewModels;

/// One shared view of the controller, polled from the engine at ~30 Hz.
/// The device header card and every page bind to App.Device, so there is
/// only one timer and one source of truth.
public partial class DeviceViewModel : ObservableObject
{
    private readonly DispatcherQueueTimer _timer;

    // ---- connection / identity ----
    [ObservableProperty] private bool _connected;
    [ObservableProperty] private string _deviceName = "No controller detected";
    [ObservableProperty] private string _profileName = "Default profile";
    [ObservableProperty] private string _kindText = "";
    [ObservableProperty] private string _transportText = "";
    [ObservableProperty] private string _transportGlyph = ""; // USB-ish; set per transport
    [ObservableProperty] private int _batteryPercent;
    [ObservableProperty] private string _batteryText = "";
    [ObservableProperty] private string _rateText = "";
    [ObservableProperty] private bool _isDualSense;
    /// "HIDDEN FROM GAMES" / "VISIBLE TO GAMES" for the title-bar pill.
    [ObservableProperty] private string _hiddenText = "";
    private int _tick;
    private int _padTick;

    /// Every connected controller, in slot order (the controller bar).
    public System.Collections.ObjectModel.ObservableCollection<PadViewModel> Pads { get; } = new();

    /// Raised when the set of controllers or the selected one changes.
    public event Action? PadsChanged;
    private int _lastSelected = -1;
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatus))]
    private string _status = "";
    public bool HasStatus => !string.IsNullOrEmpty(Status);

    // ---- live input (My Controller page) ----
    [ObservableProperty] private double _leftDotX = 54;
    [ObservableProperty] private double _leftDotY = 54;
    [ObservableProperty] private double _rightDotX = 54;
    [ObservableProperty] private double _rightDotY = 54;
    [ObservableProperty] private double _l2;
    [ObservableProperty] private double _r2;
    [ObservableProperty] private string _pressed = "";
    [ObservableProperty] private string _gyro = "";

    /// Raw state for the controller visualizer; raised every refresh (~30 Hz).
    public event Action<DxoState>? StateUpdated;
    /// Engine device kind (1 DS4, 2 DualSense, 3 Edge, 4 generic) for picking a skin.
    public byte Kind { get; private set; }

    public DeviceViewModel(DispatcherQueue dq)
    {
        _timer = dq.CreateTimer();
        _timer.Interval = TimeSpan.FromMilliseconds(33);
        _timer.Tick += (_, _) => Refresh();
        _timer.Start();
        if (App.StartupError is { } err) Status = err;
    }

    private void Refresh()
    {
        var e = App.Engine;
        if (e is null) return;

        if (_padTick++ % 8 == 0) RefreshPads(e); // ~4 times a second

        var s = e.GetState();
        Connected = s.Connected != 0;
        if (!Connected)
        {
            DeviceName = "No controller detected";
            KindText = TransportText = BatteryText = RateText = "";
            IsDualSense = false;
            Status = string.IsNullOrEmpty(e.LastError)
                ? "Connect a controller over USB or Bluetooth."
                : e.LastError;
            return;
        }

        Status = "";
        DeviceName = e.ProductName;
        KindText = s.Kind switch { 1 => "DualShock 4", 2 => "DualSense", 3 => "DualSense Edge", 4 => "Generic gamepad", 5 => "Switch Pro Controller", _ => "Unknown" };
        if (_tick++ % 15 == 0) // twice a second is plenty
            HiddenText = e.GetVirtual().PadHidden != 0 ? "HIDDEN FROM GAMES" : "VISIBLE TO GAMES";
        IsDualSense = s.Kind is 2 or 3;
        bool bt = s.Transport == 1;
        TransportText = bt ? "Bluetooth" : "USB";
        TransportGlyph = bt ? "" : ""; // Segoe Fluent: Bluetooth / USB
        BatteryPercent = s.BatteryPercent == 255 ? 0 : s.BatteryPercent;
        BatteryText = s.BatteryPercent == 255
            ? "Battery ?"
            : $"{s.BatteryPercent}%" + (s.BatteryStatus switch { 2 => " charging", 3 => " full", _ => "" });
        RateText = $"{s.ReportsPerSec} Hz";

        const double c = 54, r = 54; // 12 px dot in a 120 px circle
        LeftDotX = c + s.LX * r;
        LeftDotY = c - s.LY * r;
        RightDotX = c + s.RX * r;
        RightDotY = c - s.RY * r;
        L2 = s.L2 * 100;
        R2 = s.R2 * 100;
        Pressed = s.Buttons == 0 ? "-" : ButtonNames(s.Buttons, s.Kind);
        Gyro = $"{s.GyroX,6} {s.GyroY,6} {s.GyroZ,6}";
        Kind = s.Kind;
        StateUpdated?.Invoke(s);
    }

    /// Sync Pads with the engine: update in place, add new, drop unplugged.
    public void RefreshPads(DxoEngine e)
    {
        var list = e.ListPads();
        bool changed = false;
        for (int i = Pads.Count - 1; i >= 0; i--)
            if (!list.Exists(p => p.Slot == Pads[i].Slot)) { Pads.RemoveAt(i); changed = true; }
        foreach (var p in list)
        {
            var vm = System.Linq.Enumerable.FirstOrDefault(Pads, x => x.Slot == p.Slot);
            if (vm is null)
            {
                vm = new PadViewModel(p.Slot);
                int at = 0;
                while (at < Pads.Count && Pads[at].Slot < p.Slot) at++;
                Pads.Insert(at, vm);
                changed = true;
            }
            vm.Update(p);
        }
        int sel = e.SelectedPad;
        if (sel != _lastSelected) { _lastSelected = sel; changed = true; }
        if (changed) PadsChanged?.Invoke();
    }

    /// Point every page at this controller.
    public void SelectPad(int slot)
    {
        App.Engine?.SelectPad(slot);
        if (App.Engine is { } e) RefreshPads(e);
    }

    /// Pressed buttons as the player knows them: PlayStation names for Sony and
    /// generic pads, Nintendo letters for a Switch Pro (by position).
    private static string ButtonNames(uint bits, byte kind)
    {
        bool nintendo = kind == 5;
        var names = new (DxoButtons Bit, string Name)[]
        {
            (DxoButtons.North, nintendo ? "X" : "Triangle"), (DxoButtons.East, nintendo ? "A" : "Circle"),
            (DxoButtons.South, nintendo ? "B" : "Cross"), (DxoButtons.West, nintendo ? "Y" : "Square"),
            (DxoButtons.L1, nintendo ? "L" : "L1"), (DxoButtons.R1, nintendo ? "R" : "R1"),
            (DxoButtons.L2, nintendo ? "ZL" : "L2"), (DxoButtons.R2, nintendo ? "ZR" : "R2"),
            (DxoButtons.L3, "L3"), (DxoButtons.R3, "R3"),
            (DxoButtons.Select, nintendo ? "Minus" : "Share"), (DxoButtons.Start, nintendo ? "Plus" : "Options"),
            (DxoButtons.Home, nintendo ? "Home" : "PS"), (DxoButtons.Touchpad, nintendo ? "Capture" : "Touchpad"),
            (DxoButtons.Mute, "Mute"),
            (DxoButtons.Up, "Up"), (DxoButtons.Down, "Down"), (DxoButtons.Left, "Left"), (DxoButtons.Right, "Right"),
            (DxoButtons.PaddleL, "Left paddle"), (DxoButtons.PaddleR, "Right paddle"),
        };
        var on = new System.Collections.Generic.List<string>();
        foreach (var (bit, name) in names)
            if ((bits & (uint)bit) != 0) on.Add(name);
        return on.Count == 0 ? "-" : string.Join(", ", on);
    }

    // ---- output: one place that builds DxoOutput so pages don't fight ----
    private DxoOutput _out = new() { R = 0xB2, G = 0x6B, B = 0xFF, PlayerLeds = 0b00100 };

    public void SetLightbar(Windows.UI.Color c) { _out.R = c.R; _out.G = c.G; _out.B = c.B; Push(); }
    public void SetPlayerLeds(byte mask) { _out.PlayerLeds = (byte)(mask & 0x1F); Push(); }
    public void SetTriggers(DxoTrigger left, DxoTrigger right) { _out.TriggerLeft = left; _out.TriggerRight = right; Push(); }
    public void SetRumble(byte left, byte right) { _out.RumbleLeft = left; _out.RumbleRight = right; Push(); }
    public void UseProfileOutput() => App.Engine?.SetOutput(null);

    private void Push() => App.Engine?.SetOutput(_out);
}
