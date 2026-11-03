using System;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// Profiles, Buttons, Mouse, Motion, Rumble, Triggers and Lighting in one
/// page with tabs, so it's always clear which profile is being changed.
public sealed partial class ProfileEditorPage : Page
{
    /// Tab shown next time (remembered while the app runs; set by NavigateTo("mappings") etc.).
    public static string LastTab = "library";

    private bool _loading;

    public ProfileEditorPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
    }

    private static MainWindow? Main => ((App)Application.Current).MainWindowRef as MainWindow;

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is string tab) LastTab = tab;
        ShowTab(LastTab);

        // The controller strip stands in for the top bar here.
        PadStrip.ItemsSource = App.Device.Pads;
        App.Device.PadsChanged += OnPadsChanged;
        Main?.SetTopBarHidden(true);
        AnimateStripIn();
        UpdateHeader();
    }

    /// Leaving the Profiles page: the tab's page must get its OnNavigatedFrom too,
    /// or its timers and device events keep running in the background.
    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        App.Device.PadsChanged -= OnPadsChanged;
        Main?.SetTopBarHidden(false);
        Inner.Navigate(typeof(Page), null, new SuppressNavigationTransitionInfo());
        Inner.BackStack.Clear();
    }

    // ===== controller strip =====

    private void OnPadsChanged() => UpdateHeader();

    /// Single-click a pill: edit that controller's profile.
    private void StripPill_Tapped(object sender, TappedRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is PadViewModel pad)
        {
            App.Device.SelectPad(pad.Slot); // pages follow the selected controller
            UpdateHeader();
        }
    }

    /// Double-click a pill: open its details card (rename, emulate-as, colour...).
    private void StripPill_DoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (sender is FrameworkElement el && el.DataContext is PadViewModel pad)
            Main?.OpenControllerCard(pad, el);
    }

    /// Slide + fade the strip down from the top bar's position.
    private void AnimateStripIn()
    {
        StripHost.Opacity = 1;
        StripShift.Y = 0;
        if (App.Settings.TransitionAnimationScale <= 0 || App.Device.Pads.Count == 0) return;

        // A positional glide needs a readable duration; the global "snappy" scale
        // (0.3x) makes it pop, so floor it. One gentle ease, one direction: it
        // drops into place from where the top bar was.
        double ms = Math.Max(240, 260 * App.Settings.TransitionAnimationScale);
        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        StripHost.Opacity = 0;
        StripShift.Y = -16;
        var sb = new Storyboard();
        var fade = new DoubleAnimation { From = 0, To = 1, Duration = TimeSpan.FromMilliseconds(ms), EasingFunction = ease };
        Storyboard.SetTarget(fade, StripHost);
        Storyboard.SetTargetProperty(fade, "Opacity");
        var move = new DoubleAnimation { From = -16, To = 0, Duration = TimeSpan.FromMilliseconds(ms), EasingFunction = ease };
        Storyboard.SetTarget(move, StripShift);
        Storyboard.SetTargetProperty(move, "Y");
        sb.Children.Add(fade);
        sb.Children.Add(move);
        sb.Begin();
    }

    /// Select a tab by tag (library, buttons, mouse, motion, rumble, triggers, lighting).
    public void ShowTab(string tag)
    {
        var item = Tabs.Items.FirstOrDefault(i => (string)i.Tag == tag) ?? Tabs.Items[0];
        if (Tabs.SelectedItem == item) { Open(tag); return; }
        Tabs.SelectedItem = item; // fires Tabs_SelectionChanged -> Open
    }

    /// Bumper flip between tabs (called by controller navigation).
    public void CycleTab(int step)
    {
        int n = Tabs.Items.Count;
        if (n == 0) return;
        int i = Tabs.Items.IndexOf(Tabs.SelectedItem);
        if (i < 0) i = 0;
        Tabs.SelectedItem = Tabs.Items[((i + step) % n + n) % n];
    }

    /// The selected controller changed: show the new one's profile.
    public void ReloadInner()
    {
        if (LastTab != "library") Open(LastTab);
        else UpdateHeader();
    }

    private void Tabs_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (_loading || sender.SelectedItem?.Tag is not string tag) return;
        LastTab = tag;
        Open(tag);
    }

    private void Open(string tag)
    {
        Type page = tag switch
        {
            "buttons" => typeof(MappingsPage),
            "mouse" => typeof(MousePage),
            "motion" => typeof(MotionPage),
            "rumble" => typeof(HapticsPage),
            "triggers" => typeof(TriggersPage),
            "lighting" => typeof(LightingPage),
            _ => typeof(ProfilesPage),
        };
        Inner.Navigate(page, "embedded", new SuppressNavigationTransitionInfo());
        Inner.BackStack.Clear();
        if (Inner.Content is Page p) Embed(p);
        UpdateHeader();
    }

    /// The tab already says what this is: hide the page's own big title and
    /// tighten its top padding so it sits under the tabs.
    private static void Embed(Page p)
    {
        var content = p.Content is ScrollViewer sv ? sv.Content : p.Content;
        if (content is not Panel root) return;
        if (root is Grid g) g.Padding = new Thickness(g.Padding.Left, 8, g.Padding.Right, g.Padding.Bottom);
        else if (root is StackPanel sp) sp.Padding = new Thickness(sp.Padding.Left, 8, sp.Padding.Right, sp.Padding.Bottom);
        var title = (Style)Application.Current.Resources["DxoPageTitle"];
        HideTitle(root, title);
    }

    private static bool HideTitle(Panel panel, Style title)
    {
        foreach (var child in panel.Children)
        {
            if (child is TextBlock tb && tb.Style == title) { tb.Visibility = Visibility.Collapsed; return true; }
            if (child is Panel inner && HideTitle(inner, title)) return true;
        }
        return false;
    }

    private void UpdateHeader()
    {
        var pad = App.Device.Pads.FirstOrDefault(p => p.IsSelected) ?? App.Device.Pads.FirstOrDefault();
        string profile = ProfileStore.Active()?.Name ?? "";
        if (pad is null)
        {
            EditingText.Text = LastTab == "library"
                ? "All your saved profiles. Connect a controller to edit the one it's using live."
                : "No controller connected: plug one in to edit its profile live.";
            EditingDot.Visibility = Visibility.Collapsed;
            return;
        }
        EditingDot.Visibility = Visibility.Visible;
        int slot = Math.Clamp(pad.Slot, 0, ControllerModel.PlayerColours.Length - 1);
        EditingDot.Background = new SolidColorBrush(ControllerModel.PlayerColours[slot]);
        EditingText.Text = LastTab == "library"
            ? $"All your saved profiles. {pad.PlayerText} ({pad.Name}) is using \"{profile}\"."
            : $"Editing \"{profile}\", the profile {pad.PlayerText} ({pad.Name}) is using right now. Click another controller above to edit its profile; double-click one for its details.";
    }
}
