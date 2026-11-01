using System.Collections.ObjectModel;
using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

public sealed record Feature(string Name, string Glyph, Brush Brush);

public sealed partial class MyControllerPage : Page
{
    public DeviceViewModel Device => App.Device;
    private readonly ObservableCollection<Feature> _features = new();

    public MyControllerPage()
    {
        InitializeComponent();
        Features.ItemsSource = _features;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        Device.PropertyChanged += OnDeviceChanged;
        BuildFeatures();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => Device.PropertyChanged -= OnDeviceChanged;

    private void OnDeviceChanged(object? s, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(DeviceViewModel.KindText) or nameof(DeviceViewModel.TransportText))
            BuildFeatures();
    }

    /// Which features this pad supports. Unsupported ones are shown, not hidden,
    /// so the user can see why an option is greyed out elsewhere.
    private void BuildFeatures()
    {
        bool connected = Device.Connected;
        bool ds = Device.IsDualSense;
        bool usb = Device.TransportText == "USB";

        _features.Clear();
        Add("Light bar", connected);
        Add("Rumble", connected);
        Add("Touchpad", connected);
        Add("Motion (gyro + accelerometer)", connected);
        Add("Adaptive triggers", connected && ds);
        Add("Player LEDs", connected && ds);
        Add("Mute button", connected && ds);
        Add("Voice-coil haptics (USB only)", connected && ds && usb);
        Add("Speaker / headset audio", connected && usb);
    }

    private void Add(string name, bool ok)
    {
        var res = Application.Current.Resources;
        _features.Add(new Feature(name, ok ? "" : "", (Brush)res[ok ? "DxoSuccess" : "DxoTextSecondary"]));
    }
}
