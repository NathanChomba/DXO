using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml;
using DXO.UI.Native;

namespace DXO.UI.ViewModels;

/// One controller pill in the controller bar. Updated in place (by slot) so
/// the pills don't flicker while numbers change.
public partial class PadViewModel : ObservableObject
{
    public int Slot { get; }
    /// Stable hardware id: settings follow the controller, not the slot.
    public string Id { get; private set; } = "";

    [ObservableProperty] private string _name = "";
    [ObservableProperty] private string _kindText = "";
    [ObservableProperty] private bool _bluetooth;
    [ObservableProperty] private string _transportGlyph = "";
    [ObservableProperty] private string _transportText = "";
    [ObservableProperty] private string _rateText = "";
    [ObservableProperty] private string _batteryText = "";
    /// Width of the battery gauge's fill, 0..14 px.
    [ObservableProperty] private double _batteryFill;
    [ObservableProperty] private bool _batteryLow;
    [ObservableProperty] private bool _charging;
    [ObservableProperty] private bool _hidden;
    [ObservableProperty] private Visibility _hiddenVisibility = Visibility.Collapsed;
    [ObservableProperty] private string _profile = "";
    [ObservableProperty] private bool _isSelected;
    [ObservableProperty] private string _gamesSee = "";
    [ObservableProperty] private byte _target;
    [ObservableProperty] private string _playerText = "";
    /// The controller's lightbar colour, shown as the pill's dot.
    [ObservableProperty] private Microsoft.UI.Xaml.Media.SolidColorBrush _lightBrush = new(Microsoft.UI.Colors.Gray);
    /// Light colour needs re-reading (new controller in this slot, profile or setting changed).
    public bool LightStale { get; set; } = true;

    /// Accent ring when this is the controller being edited, else none.
    /// Used by the Profiles-page controller strip to show the selection.
    public Microsoft.UI.Xaml.Media.Brush SelectionBrush => IsSelected
        ? (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["DxoAccent"]
        : Transparent;

    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush Transparent = new(Microsoft.UI.Colors.Transparent);

    partial void OnIsSelectedChanged(bool value) => OnPropertyChanged(nameof(SelectionBrush));

    public PadViewModel(int slot) => Slot = slot;

    public void Update(PadInfo p)
    {
        if (Id != p.Id || Profile != p.Profile) LightStale = true;
        Id = p.Id;
        Name = p.Name.Length == 0 ? "Controller" : p.Name;
        KindText = p.Kind switch { 1 => "DualShock 4", 2 => "DualSense", 3 => "DualSense Edge", 4 => "Generic gamepad", 5 => "Switch Pro Controller", _ => "Controller" };
        Bluetooth = p.Bluetooth;
        TransportGlyph = p.Bluetooth ? "" : ""; // Segoe Fluent: Bluetooth / USB
        TransportText = p.Bluetooth ? "Bluetooth" : "USB";
        RateText = $"{p.ReportsPerSec} Hz";
        bool known = p.BatteryPercent != 255;
        Charging = p.BatteryStatus is 2 or 3;
        BatteryText = !known ? "--" : Charging ? $"{p.BatteryPercent}% ⚡" : $"{p.BatteryPercent}%";
        BatteryFill = known ? System.Math.Max(1, 14.0 * p.BatteryPercent / 100.0) : 0;
        BatteryLow = known && !Charging && p.BatteryPercent <= 15;
        Hidden = p.Hidden;
        HiddenVisibility = p.Hidden ? Visibility.Visible : Visibility.Collapsed;
        Profile = p.Profile;
        IsSelected = p.Selected;
        Target = p.Target;
        GamesSee = !p.VirtualActive ? "Your real controller" : p.Target == 2 ? "DualShock 4" : "Xbox 360";
        PlayerText = $"Player {p.Slot + 1}";
    }
}
