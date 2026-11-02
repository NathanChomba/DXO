using System;
using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using DXO.UI.Services;
using DXO.UI.ViewModels;
using DXO.UI.Views;
using Microsoft.UI.Windowing;

namespace DXO.UI;

public sealed partial class MainWindow : Window
{
    public DeviceViewModel Device => App.Device;

    private TrayIcon? _tray;
    private bool _exiting;

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 800));

        string icon = System.IO.Path.Combine(AppContext.BaseDirectory, "dxo.ico");
        if (System.IO.File.Exists(icon)) AppWindow.SetIcon(icon);

        try
        {
            _tray = new TrayIcon(WinRT.Interop.WindowNative.GetWindowHandle(this), icon, "DXO");
            _tray.OpenRequested += ShowFromTray;
            _tray.ExitRequested += ExitApp;
        }
        catch (Exception ex) { App.LogCrash("Tray", ex); } // no tray: closing just closes

        AppWindow.Closing += OnClosing;
        AppWindow.Changed += OnWindowChanged;
        Closed += (_, _) => _tray?.Dispose();
    }

    // ================= tray =================

    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (_exiting || _tray is null || !App.Settings.CloseToTray) return;
        args.Cancel = true;
        HideToTray();
    }

    private void OnWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (_tray is not null && App.Settings.MinimizeToTray && sender.IsVisible &&
            sender.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized })
            HideToTray();
    }

    private void HideToTray()
    {
        AppWindow.Hide();
        if (!App.Settings.TrayHintShown)
        {
            _tray?.ShowBalloon("DXO is still running", "Your controller keeps working. Click the DXO icon to open it, or right-click to exit.");
            App.Settings.TrayHintShown = true;
            App.Settings.Save();
        }
    }

    public void ShowFromTray()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } p) p.Restore();
        AppWindow.Show();
        Activate();
    }

    /// Really quit: close the window for good (App disposes the engine, which unhides the pad).
    public void ExitApp()
    {
        _exiting = true;
        Close();
    }

    public void SetTrayTip(string tip) => _tray?.SetTip(tip);

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
        if (e.PropertyName is nameof(DeviceViewModel.Connected) or nameof(DeviceViewModel.DeviceName) or nameof(DeviceViewModel.ProfileName))
            SetTrayTip(Device.Connected ? $"DXO: {Device.DeviceName} ({Device.ProfileName})" : "DXO: no controller");
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
            case "mappings": Go(typeof(MappingsPage)); break;
            case "diagnostics": Go(typeof(DiagnosticsPage)); break;
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
