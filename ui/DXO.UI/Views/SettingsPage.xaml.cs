using System;
using System.Diagnostics;
using System.IO;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace DXO.UI.Views;

public sealed partial class SettingsPage : Page
{
    private const string HidHideCli = @"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";

    public SettingsPage()
    {
        InitializeComponent();
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

    private void OpenProfiles_Click(object sender, RoutedEventArgs e)
    {
        var dir = Path.Combine(AppContext.BaseDirectory, "profiles");
        Directory.CreateDirectory(dir);
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"") { UseShellExecute = true });
    }
}
