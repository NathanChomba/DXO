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
        KindText = s.Kind switch { 1 => "DualShock 4", 2 => "DualSense", 3 => "DualSense Edge", 4 => "Generic gamepad", _ => "Unknown" };
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
        Pressed = s.Buttons == 0 ? "-" : ((DxoButtons)s.Buttons).ToString();
        Gyro = $"{s.GyroX,6} {s.GyroY,6} {s.GyroZ,6}";
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
