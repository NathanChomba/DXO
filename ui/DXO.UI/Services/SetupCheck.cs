using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace DXO.UI.Services;

/// First-run setup: are the two drivers there, and is a controller connected?
/// Shown once on first start, and again from Settings > Run setup.
public static class SetupCheck
{
    public const string HidHideCli = @"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";
    private const string VigemUrl = "https://github.com/nefarius/ViGEmBus/releases/latest";
    private const string HidHideUrl = "https://github.com/nefarius/HidHide/releases/latest";

    public static bool VigemInstalled => File.Exists(Path.Combine(Environment.SystemDirectory, "drivers", "ViGEmBus.sys"));
    public static bool HidHideInstalled => File.Exists(HidHideCli);
    public static bool ControllerConnected => App.Device?.Pads.Count > 0;

    public static async Task ShowAsync(XamlRoot root)
    {
        var rows = new StackPanel { Spacing = 14, MinWidth = 440 };
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = "Set up DXO",
            Content = new ScrollViewer { Content = rows },
            PrimaryButtonText = "Done",
            SecondaryButtonText = "Check again",
            CloseButtonText = "Skip for now",
            DefaultButton = ContentDialogButton.Primary,
        };
        void fill()
        {
            rows.Children.Clear();
            rows.Children.Add(new TextBlock
            {
                Text = "DXO needs two free drivers from Nefarius. Install any that are missing, then press Check again.",
                TextWrapping = TextWrapping.Wrap, Opacity = 0.8,
            });
            rows.Children.Add(Row(VigemInstalled, "Virtual controller driver (ViGEmBus)",
                "Lets games see DXO's Xbox 360 or DualShock 4 controller.", "Download", () => Open(VigemUrl)));
            rows.Children.Add(Row(HidHideInstalled, "Controller hiding (HidHide)",
                "Stops games also seeing your real controller, so there's no double input. Restart the PC after installing it.",
                "Download", () => Open(HidHideUrl)));
            rows.Children.Add(Row(ControllerConnected, "Connect a controller",
                "Plug it in with a USB cable, or pair it over Bluetooth: hold PS + Share until the light flashes.",
                "Bluetooth settings", () => Open("ms-settings:bluetooth")));
            bool ready = VigemInstalled && HidHideInstalled && ControllerConnected;
            dialog.PrimaryButtonText = ready ? "Start using DXO" : "Done";
        }
        fill();
        dialog.SecondaryButtonClick += (_, e) => { e.Cancel = true; fill(); }; // stay open, refresh
        await dialog.ShowAsync();
        App.Settings.SetupDone = true;
        App.Settings.Save();
    }

    private static FrameworkElement Row(bool ok, string title, string text, string action, Action click)
    {
        var g = new Grid { ColumnSpacing = 12 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var res = Application.Current.Resources;
        var icon = new FontIcon
        {
            Glyph = ok ? "\uE73E" : "\uE7BA", FontSize = 18, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 2, 0, 0),
            Foreground = (Brush)res[ok ? "DxoSuccess" : "DxoWarning"],
        };
        var body = new StackPanel { Spacing = 2 };
        body.Children.Add(new TextBlock { Text = title, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold });
        body.Children.Add(new TextBlock { Text = ok ? "Ready." : text, TextWrapping = TextWrapping.Wrap, Opacity = 0.75 });
        Grid.SetColumn(body, 1);
        g.Children.Add(icon);
        g.Children.Add(body);
        if (!ok)
        {
            var b = new Button { Content = action, VerticalAlignment = VerticalAlignment.Center };
            b.Click += (_, _) => click();
            Grid.SetColumn(b, 2);
            g.Children.Add(b);
        }
        return g;
    }

    public static void Open(string url)
    {
        try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
        catch (Exception ex) { App.LogCrash($"Open {url}", ex); }
    }
}
