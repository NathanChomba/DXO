using System;
using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using DXO.UI.ViewModels;
using DXO.UI.Views;

namespace DXO.UI;

public sealed partial class MainWindow : Window
{
    public DeviceViewModel Device => App.Device;

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 800));
        // TODO: AppWindow.SetIcon("Assets/dxo.ico") once there is an icon file
    }

    /// Called by App once App.Device exists, so x:Bind sees a non-null source.
    public void Start()
    {
        Bindings.Update();
        Device.PropertyChanged += OnDeviceChanged;
        UpdateStatusRing();
        Nav.SelectedItem = Nav.MenuItems[0];
    }

    private void OnDeviceChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(DeviceViewModel.Connected)) UpdateStatusRing();
    }

    private void UpdateStatusRing() =>
        StatusRing.Stroke = (Brush)Application.Current.Resources[Device.Connected ? "DxoSuccess" : "DxoDanger"];

    private void DeviceCard_Click(object sender, RoutedEventArgs e) => Select("controller");

    /// Lets pages jump to another page by nav tag, e.g. Home's quick-action tiles.
    public static void NavigateTo(string tag) =>
        ((MainWindow?)((App)Application.Current).MainWindowRef)?.Select(tag);

    public void OpenSettings() => Nav.SelectedItem = Nav.SettingsItem;

    private void Select(string tag)
    {
        foreach (var item in Nav.MenuItems)
            if (item is NavigationViewItem n && (string)n.Tag == tag) { Nav.SelectedItem = n; return; }
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.IsSettingsSelected) { Go(typeof(SettingsPage)); return; }

        switch (args.SelectedItemContainer?.Tag as string)
        {
            case "home": Go(typeof(HomePage)); break;
            case "controller": Go(typeof(MyControllerPage)); break;
            case "profiles": Go(typeof(ProfilesPage)); break;
            case "virtual": Go(typeof(VirtualDevicePage)); break;
            case "triggers": Go(typeof(TriggersPage)); break;
            case "lighting": Go(typeof(LightingPage)); break;
            case "mappings": Soon("Mappings", "Click a button on the controller diagram, then choose what it does: another button, a key, the mouse, a shortcut, turbo or toggle.", "Milestone 5"); break;
            case "haptics": Soon("Haptics and Rumble", "Rumble intensity, rumble passthrough from games, and audio-driven haptics.", "Milestone 3 (rumble), Milestone 6 (audio haptics)"); break;
            case "motion": Go(typeof(MotionPage)); break;
            case "games": Soon("Games", "Detected games with per-game profiles that switch automatically.", "Milestone 5"); break;
            case "about": Go(typeof(AboutPage)); break;
        }
    }

    private void Go(Type page)
    {
        if (ContentFrame.CurrentSourcePageType != page)
            ContentFrame.Navigate(page);
    }

    private void Soon(string title, string what, string when) =>
        ContentFrame.Navigate(typeof(PlaceholderPage), new PlaceholderInfo(title, what, when));
}
