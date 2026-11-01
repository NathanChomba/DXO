using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Native;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

public sealed partial class TriggersPage : Page, INotifyPropertyChanged
{
    public DeviceViewModel Device => App.Device;
    public bool NotDualSense => !Device.IsDualSense;
    public double PanelOpacity => Device.IsDualSense ? 1.0 : 0.45;
    public event PropertyChangedEventHandler? PropertyChanged;

    private bool _ready;

    public TriggersPage()
    {
        InitializeComponent();
        _ready = true;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e) => Device.PropertyChanged += OnDevice;
    protected override void OnNavigatedFrom(NavigationEventArgs e) => Device.PropertyChanged -= OnDevice;

    private void OnDevice(object? s, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(DeviceViewModel.IsDualSense)) return;
        PropertyChanged?.Invoke(this, new(nameof(NotDualSense)));
        PropertyChanged?.Invoke(this, new(nameof(PanelOpacity)));
    }

    private void Changed(object sender, object e)
    {
        if (!_ready) return; // controls fire events while InitializeComponent runs
        Device.SetTriggers(
            Build(LeftMode.SelectedIndex, LeftStart.Value, LeftEnd.Value, LeftStrength.Value),
            Build(RightMode.SelectedIndex, RightStart.Value, RightEnd.Value, RightStrength.Value));
    }

    private static DxoTrigger Build(int mode, double start, double end, double strength) => new()
    {
        Mode = (byte)mode, // 0 off, 1 feedback, 2 weapon - matches ffi.rs DxoTrigger
        Start = (byte)start,
        End = (byte)end,
        Strength = (byte)strength,
    };

    private void Reset_Click(object sender, RoutedEventArgs e) => Device.UseProfileOutput();
}
