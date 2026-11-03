using System;
using System.Diagnostics;
using System.IO;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using DXO.UI.Services;

namespace DXO.UI.Views;

public sealed partial class SettingsPage : Page
{
    private const string HidHideCli = SetupCheck.HidHideCli;

    private bool _loading;

    public SettingsPage()
    {
        _loading = true;
        InitializeComponent();
        CloseToggle.IsOn = App.Settings.CloseToTray;
        MinToggle.IsOn = App.Settings.MinimizeToTray;
        StartToggle.IsOn = Autostart.IsEnabled;
        GenericToggle.IsOn = App.Settings.IncludeGeneric;
        NavToggle.IsOn = App.Settings.ControllerNavigation;
        UpdateSourceBox.Text = App.Settings.UpdateSource;
        UpdateToggle.IsOn = App.Settings.CheckUpdatesOnStart;
        UpdateCard.Description = $"You have DXO {App.Version}.";
        foreach (var box in new[] { WindowScaleBox, TransitionScaleBox, AnimatorScaleBox })
            box.ItemsSource = AppSettings.ScaleLabels;
        WindowScaleBox.SelectedIndex = ScaleIndex(App.Settings.WindowAnimationScale);
        TransitionScaleBox.SelectedIndex = ScaleIndex(App.Settings.TransitionAnimationScale);
        AnimatorScaleBox.SelectedIndex = ScaleIndex(App.Settings.AnimatorDurationScale);
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

    /// Closest offered choice to a saved value.
    private static int ScaleIndex(double v)
    {
        int best = 0;
        for (int i = 1; i < AppSettings.ScaleChoices.Length; i++)
            if (Math.Abs(AppSettings.ScaleChoices[i] - v) < Math.Abs(AppSettings.ScaleChoices[best] - v)) best = i;
        return best;
    }

    private void Scale_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        double Pick(ComboBox b) => AppSettings.ScaleChoices[Math.Max(0, b.SelectedIndex)];
        App.Settings.WindowAnimationScale = Pick(WindowScaleBox);
        App.Settings.TransitionAnimationScale = Pick(TransitionScaleBox);
        App.Settings.AnimatorDurationScale = Pick(AnimatorScaleBox);
        App.Settings.Save();
    }

    private void Generic_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        App.Settings.IncludeGeneric = GenericToggle.IsOn;
        App.Settings.Save();
        App.Engine?.SetIncludeGeneric(GenericToggle.IsOn);
    }

    private void Nav_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        App.Settings.ControllerNavigation = NavToggle.IsOn;
        App.Settings.Save();
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

    private async void RunSetup_Click(object sender, RoutedEventArgs e)
    {
        try { await SetupCheck.ShowAsync(XamlRoot); }
        catch (Exception ex) { App.LogCrash("RunSetup", ex); }
    }

    private void UpdateSource_LostFocus(object sender, RoutedEventArgs e)
    {
        App.Settings.UpdateSource = UpdateSourceBox.Text.Trim();
        App.Settings.Save();
    }

    private void UpdateToggle_Toggled(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        App.Settings.CheckUpdatesOnStart = UpdateToggle.IsOn;
        App.Settings.Save();
    }

    private async void CheckUpdates_Click(object sender, RoutedEventArgs e)
    {
        UpdateSource_LostFocus(sender, e);
        CheckButton.IsEnabled = false;
        UpdateCard.Description = "Checking...";
        try
        {
            var (rel, err) = await Updater.CheckAsync(App.Settings.UpdateSource);
            if (err is not null) UpdateCard.Description = err;
            else if (rel is null) UpdateCard.Description = $"You have the latest version ({App.Version}).";
            else
            {
                UpdateCard.Description = $"DXO {rel.Version} is available.";
                if (((App)Application.Current).MainWindowRef is MainWindow w) await w.OfferUpdate(rel);
            }
        }
        catch (Exception ex) { App.LogCrash("CheckUpdates", ex); UpdateCard.Description = "Couldn't check."; }
        finally { CheckButton.IsEnabled = true; }
    }

    private void OpenProfiles_Click(object sender, RoutedEventArgs e)
    {
        var dir = App.ProfilesDir;
        Directory.CreateDirectory(dir);
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"") { UseShellExecute = true });
    }
}
