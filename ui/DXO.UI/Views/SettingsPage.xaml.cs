using System;
using System.Diagnostics;
using System.IO;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using DXO.UI.Services;

namespace DXO.UI.Views;

public sealed partial class SettingsPage : Page
{
    private const string HidHideCli = @"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";

    private bool _loading;

    public SettingsPage()
    {
        _loading = true;
        InitializeComponent();
        CloseToggle.IsOn = App.Settings.CloseToTray;
        MinToggle.IsOn = App.Settings.MinimizeToTray;
        StartToggle.IsOn = Autostart.IsEnabled;
        GenericToggle.IsOn = App.Settings.IncludeGeneric;
        _loading = false;
        UpdateStartNote();
        // Cheap install checks: look for the files each driver ships.
        bool vigem = File.Exists(Path.Combine(Environment.SystemDirectory, "drivers", "ViGEmBus.sys"));
        bool hidhide = File.Exists(HidHideCli);
        VigemStatus.Text = vigem ? "Installed" : "Not found";
        HidHideStatus.Text = hidhide ? "Installed" : "Not found";
        VigemStatus.Foreground = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources[vigem ? "DxoSuccess" : "DxoDanger"];
        HidHideStatus.Foreground = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources[hidhide ? "DxoSuccess" : "DxoDanger"];
    }

    private async void UnhideAll_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Unhide all controllers?",
            Content = "Games may see your controller twice while DXO is running until it hides it again.",
            PrimaryButtonText = "Unhide all",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dlg.ShowAsync() != ContentDialogResult.Primary) return;
        if (!File.Exists(HidHideCli)) return;
        // Turning cloaking off is the "unhide everything" switch; DXO turns it back on at next connect.
        Process.Start(new ProcessStartInfo(HidHideCli, "--cloak-off") { CreateNoWindow = true, UseShellExecute = false });
    }

    private void Tray_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        App.Settings.CloseToTray = CloseToggle.IsOn;
        App.Settings.MinimizeToTray = MinToggle.IsOn;
        App.Settings.Save();
    }

    private void Generic_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        App.Settings.IncludeGeneric = GenericToggle.IsOn;
        App.Settings.Save();
        App.Engine?.SetIncludeGeneric(GenericToggle.IsOn);
    }

    private void Start_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        var err = Autostart.Set(StartToggle.IsOn);
        if (err is not null)
        {
            _loading = true;
            StartToggle.IsOn = Autostart.IsEnabled;
            _loading = false;
        }
        UpdateStartNote(err);
    }

    private void UpdateStartNote(string? error = null)
    {
        var res = Application.Current.Resources;
        if (error is not null)
        {
            StartNote.Text = "Couldn't change it: " + error;
            StartNote.Foreground = (Microsoft.UI.Xaml.Media.Brush)res["DxoDanger"];
            return;
        }
        StartNote.Foreground = (Microsoft.UI.Xaml.Media.Brush)res["DxoTextSecondary"];
        if (Autostart.HasTask) StartNote.Text = "Starts as administrator, so it can hide your controller from games.";
        else if (Autostart.HasRunKey) StartNote.Text = Autostart.IsElevated
            ? "Turn this off and on again to switch to the administrator startup (needed to hide your controller)."
            : "Starts without administrator rights, so your controller won't be hidden at sign-in. Turn this on while DXO runs as administrator to fix that.";
        else StartNote.Text = Autostart.IsElevated ? "" : "Tip: turn this on while DXO runs as administrator, so it can also hide your controller at sign-in.";
    }

    private void OpenProfiles_Click(object sender, RoutedEventArgs e)
    {
        var dir = App.ProfilesDir;
        Directory.CreateDirectory(dir);
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"") { UseShellExecute = true });
    }
}
