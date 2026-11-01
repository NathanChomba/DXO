using System.ComponentModel;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.ViewModels;
using Windows.UI;

namespace DXO.UI.Views;

public sealed partial class LightingPage : Page, INotifyPropertyChanged
{
    public DeviceViewModel Device => App.Device;
    public bool NotDualSense => !Device.IsDualSense;
    public event PropertyChangedEventHandler? PropertyChanged;

    // Bit masks for the 5 DualSense player LEDs, left to right = bit 4..0
    private static readonly byte[] PlayerMasks = { 0b00000, 0b00100, 0b01010, 0b10101, 0b11011, 0b11111 };

    private static readonly Color[] SwatchColors =
    {
        ColorHelper.FromArgb(255, 0xFF, 0x3B, 0x30), ColorHelper.FromArgb(255, 0xFF, 0x95, 0x00),
        ColorHelper.FromArgb(255, 0xFF, 0xD6, 0x0A), ColorHelper.FromArgb(255, 0x34, 0xC7, 0x59),
        ColorHelper.FromArgb(255, 0x00, 0xC7, 0xBE), ColorHelper.FromArgb(255, 0x0A, 0x84, 0xFF),
        ColorHelper.FromArgb(255, 0xB2, 0x6B, 0xFF), ColorHelper.FromArgb(255, 0xFF, 0x2D, 0x92),
        ColorHelper.FromArgb(255, 0xFF, 0xFF, 0xFF),
    };

    private bool _ready;

    public LightingPage()
    {
        InitializeComponent();
        var brushes = new System.Collections.Generic.List<SolidColorBrush>();
        foreach (var c in SwatchColors) brushes.Add(new SolidColorBrush(c));
        Swatches.ItemsSource = brushes;
        _ready = true;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e) => Device.PropertyChanged += OnDevice;
    protected override void OnNavigatedFrom(NavigationEventArgs e) => Device.PropertyChanged -= OnDevice;

    private void OnDevice(object? s, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(DeviceViewModel.IsDualSense))
            PropertyChanged?.Invoke(this, new(nameof(NotDualSense)));
    }

    private void Tabs_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        bool light = sender.SelectedItem == sender.Items[0];
        LightBarTab.Visibility = light ? Visibility.Visible : Visibility.Collapsed;
        PlayerTab.Visibility = light ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Picker_ColorChanged(ColorPicker sender, ColorChangedEventArgs args) => Apply();
    private void Brightness_ValueChanged(object sender, Microsoft.UI.Xaml.Controls.Primitives.RangeBaseValueChangedEventArgs e) => Apply();

    private void Swatch_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).Background is SolidColorBrush b) Picker.Color = b.Color; // triggers Apply via ColorChanged
    }

    /// The pad has no separate brightness control, so scale the colour instead.
    private void Apply()
    {
        if (!_ready) return;
        double k = Brightness.Value / 100.0;
        var c = Picker.Color;
        Device.SetLightbar(ColorHelper.FromArgb(255, (byte)(c.R * k), (byte)(c.G * k), (byte)(c.B * k)));
    }

    private void PlayerLeds_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_ready || PlayerLeds.SelectedIndex < 0) return;
        Device.SetPlayerLeds(PlayerMasks[PlayerLeds.SelectedIndex]);
    }

    private void Reset_Click(object sender, RoutedEventArgs e) => Device.UseProfileOutput();
}
